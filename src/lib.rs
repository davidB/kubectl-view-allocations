mod collect;
mod display;
mod metrics;
pub mod qty;
mod sort;
mod tree;

pub use collect::{collect_from_metrics, collect_from_nodes, collect_from_pods};
pub use display::{display_as_csv, display_with_prettytable};
pub use sort::{SortColumn, SortColumnName, SortDirection, parse_sort_spec};
use sort::{effective_sort_spec, flatten_tree, sort_children_recursive};

use clap::{Parser, ValueEnum};
use core::convert::TryFrom;
use itertools::Itertools;
use qty::Qty;
use std::path::PathBuf;
use tracing::{info, instrument, warn};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to run '{cmd}'")]
    CmdError {
        cmd: String,
        output: Option<std::process::Output>,
        source: Option<std::io::Error>,
    },

    #[error("Failed to read Qty of location {location:?} / {qualifier:?} {kind}={input}")]
    ResourceQtyParseError {
        location: Location,
        qualifier: ResourceQualifier,
        kind: String,
        input: String,
        source: qty::Error,
    },

    #[error("Failed to process Qty")]
    QtyError {
        #[from]
        source: qty::Error,
    },

    #[error("Failed to {context}")]
    KubeError {
        context: String,
        source: kube::Error,
    },

    #[error("Failed to {context}")]
    KubeConfigError {
        context: String,
        source: kube::config::KubeconfigError,
    },

    #[error("Failed to {context}")]
    KubeInferConfigError {
        context: String,
        source: kube::config::InferConfigError,
    },

    #[error(
        "Invalid sort column '{name}'. Valid: utilization/usage, requested, limit/limits, allocatable, free, name"
    )]
    InvalidSortColumn { name: String },
}

#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Location {
    pub node_name: String,
    pub namespace: Option<String>,
    pub pod_name: Option<String>,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Resource {
    pub kind: String,
    pub quantity: Qty,
    pub location: Location,
    pub qualifier: ResourceQualifier,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ResourceQualifier {
    Limit,
    Requested,
    Allocatable,
    Utilization,
    // HACK special qualifier, used to show zero/undef cpu & memory
    Present,
}

#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct QtyByQualifier {
    pub limit: Option<Qty>,
    pub requested: Option<Qty>,
    pub allocatable: Option<Qty>,
    pub utilization: Option<Qty>,
    pub present: Option<Qty>,
}

impl QtyByQualifier {
    pub fn calc_free(&self, used_mode: UsedMode) -> Option<Qty> {
        let total_used = match used_mode {
            UsedMode::MaxRequestLimit => {
                std::cmp::max(self.limit.as_ref(), self.requested.as_ref())
            }
            UsedMode::OnlyRequest => self.requested.as_ref(),
        };
        self.allocatable
            .as_ref()
            .zip(total_used)
            .map(|(allocatable, total_used)| {
                if allocatable > total_used {
                    allocatable - total_used
                } else {
                    Qty::default()
                }
            })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TableNode {
    pub(crate) key: String,
    pub(crate) path: Vec<String>,
    pub(crate) quantities: Option<QtyByQualifier>,
    pub(crate) free: Option<Qty>,
    pub(crate) children: Vec<usize>,
}

#[derive(Debug, Eq, PartialEq, ValueEnum, Clone)]
#[non_exhaustive]
#[value(rename_all = "snake_case")]
pub enum GroupBy {
    Resource,
    Node,
    Pod,
    Namespace,
}

impl GroupBy {
    pub fn to_fct(&self) -> fn(&Resource) -> Option<String> {
        match self {
            Self::Resource => Self::extract_kind,
            Self::Node => Self::extract_node_name,
            Self::Pod => Self::extract_pod_name,
            Self::Namespace => Self::extract_namespace,
        }
    }

    fn extract_kind(e: &Resource) -> Option<String> {
        Some(e.kind.clone())
    }

    fn extract_node_name(e: &Resource) -> Option<String> {
        Some(e.location.node_name.to_string()).filter(|s| !s.is_empty())
    }

    fn extract_pod_name(e: &Resource) -> Option<String> {
        // We do not need to display "pods" resource types when grouping by pods
        if e.kind == "pods" {
            return None;
        }
        e.location.pod_name.clone()
    }

    fn extract_namespace(e: &Resource) -> Option<String> {
        e.location.namespace.clone()
    }
}

impl std::fmt::Display for GroupBy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Resource => "resource",
            Self::Node => "node",
            Self::Pod => "pod",
            Self::Namespace => "namespace",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Eq, PartialEq, ValueEnum, Clone, Copy, Default)]
#[non_exhaustive]
#[value(rename_all = "snake_case")]
pub enum Output {
    #[default]
    Table,
    Csv,
}

#[derive(Debug, Eq, PartialEq, ValueEnum, Clone, Copy, Default)]
#[non_exhaustive]
#[value(rename_all = "snake_case")]
pub enum UsedMode {
    #[default]
    MaxRequestLimit,
    OnlyRequest,
}

#[derive(Parser, Debug)]
#[non_exhaustive]
#[command(
    version, about,
    after_help(env!("CARGO_PKG_HOMEPAGE")),
    propagate_version = true
)]
pub struct CliOpts {
    /// Path to the kubeconfig file to use for requests to kubernetes cluster
    #[arg(long, value_parser)]
    pub kubeconfig: Option<PathBuf>,

    /// The name of the kubeconfig context to use
    #[arg(long, value_parser)]
    pub context: Option<String>,

    /// Filter pods by namespace(s), by default pods in all namespaces are listed (comma separated list or multiple calls)
    #[arg(short, long, value_parser, value_delimiter= ',', num_args = 1..)]
    pub namespace: Vec<String>,

    /// Show only nodes match this label selector
    #[arg(short = 'l', long, value_parser)]
    pub selector: Option<String>,

    /// Ignore nodes with specific taints; when not specified, only nodes without taints are shown; when used without values, show all nodes (comma-separated list)
    #[arg(long, value_parser, value_delimiter = ',', num_args = 0..)]
    pub ignore_taints: Option<Vec<String>>,

    /// Force to retrieve utilization (for cpu and memory), requires
    /// having metrics-server https://github.com/kubernetes-sigs/metrics-server
    #[arg(short = 'u', long, value_parser)]
    pub utilization: bool,

    /// Show lines with zero requested AND zero limit AND zero allocatable,
    /// OR pods with unset requested AND limit for `cpu` and `memory`
    #[arg(short = 'z', long, value_parser)]
    pub show_zero: bool,

    /// The way to compute the `used` part for free (`allocatable - used`)
    #[arg(
        long,
        value_enum,
        ignore_case = true,
        default_value = "max_request_limit",
        value_parser
    )]
    pub used_mode: UsedMode,

    /// Pre-check access and refresh token on kubeconfig by running `kubectl cluster-info`
    #[arg(long, value_parser)]
    pub precheck: bool,

    /// Accept invalid certificates (dangerous)
    #[arg(long, value_parser)]
    pub accept_invalid_certs: bool,

    /// Filter resources shown by name(s), by default all resources are listed (comma separated list or multiple calls)
    #[arg(short, long, value_parser, value_delimiter= ',', num_args = 1..)]
    pub resource_name: Vec<String>,

    /// Group information in a hierarchical manner; defaults to `-g resource,node,pod` (comma-separated list or multiple calls)
    #[arg(short, long, value_enum, ignore_case = true, value_parser, value_delimiter= ',', num_args = 1..)]
    pub group_by: Vec<GroupBy>,

    /// Output format
    #[arg(
        short,
        long,
        value_enum,
        ignore_case = true,
        default_value = "table",
        value_parser
    )]
    pub output: Output,

    /// Sort rows by column(s), SQL-like syntax: 'col [ASC|DESC]' (comma-separated).
    /// Valid columns: usage/utilization, requested, limits/limit, allocatable, free, name.
    /// Direction is optional (default ASC). name ASC is always the implicit final tiebreaker.
    #[arg(
        short,
        long,
        default_value = "usage DESC, requested DESC, limits DESC, name ASC"
    )]
    pub sort: String,
}

impl CliOpts {
    pub fn effective_group_by(&self) -> Vec<GroupBy> {
        let mut group_by = if self.group_by.is_empty() {
            vec![GroupBy::Resource, GroupBy::Node, GroupBy::Pod]
        } else {
            self.group_by.clone()
        };
        if !group_by.contains(&GroupBy::Resource) {
            group_by.insert(0, GroupBy::Resource);
        }
        group_by.dedup();
        group_by
    }
}

fn add(lhs: Option<Qty>, rhs: &Qty) -> Option<Qty> {
    lhs.map(|l| &l + rhs).or_else(|| Some(rhs.clone()))
}

fn sum_by_qualifier(rsrcs: &[&Resource]) -> Option<QtyByQualifier> {
    if !rsrcs.is_empty() {
        let kind = rsrcs
            .first()
            .expect("group contains at least 1 element")
            .kind
            .clone();

        if rsrcs.iter().all(|i| i.kind == kind) {
            let sum = rsrcs.iter().fold(QtyByQualifier::default(), |mut acc, v| {
                match &v.qualifier {
                    ResourceQualifier::Limit => acc.limit = add(acc.limit, &v.quantity),
                    ResourceQualifier::Requested => acc.requested = add(acc.requested, &v.quantity),
                    ResourceQualifier::Allocatable => {
                        acc.allocatable = add(acc.allocatable, &v.quantity)
                    }
                    ResourceQualifier::Utilization => {
                        acc.utilization = add(acc.utilization, &v.quantity)
                    }
                    ResourceQualifier::Present => acc.present = add(acc.present, &v.quantity),
                };
                acc
            });
            Some(sum)
        } else {
            None
        }
    } else {
        None
    }
}

pub fn make_qualifiers(
    rsrcs: &[Resource],
    group_by: &[GroupBy],
    resource_names: &[String],
    sort_spec: &[SortColumn],
    used_mode: UsedMode,
) -> Vec<(Vec<String>, Option<QtyByQualifier>, Option<Qty>)> {
    let group_by_fct = group_by.iter().map(GroupBy::to_fct).collect::<Vec<_>>();
    let mut nodes: Vec<TableNode> = vec![];
    let mut root_indices = make_group_x_qualifier(
        &(rsrcs
            .iter()
            .filter(|a| accept_resource(&a.kind, resource_names))
            .collect::<Vec<_>>()),
        &[],
        &group_by_fct,
        0,
        &mut nodes,
        used_mode,
    );
    let resource_depth = group_by
        .iter()
        .position(|g| *g == GroupBy::Resource)
        .unwrap_or(0);
    sort_children_recursive(&mut nodes, &mut root_indices, 0, resource_depth, sort_spec);
    flatten_tree(&nodes, &root_indices)
}

fn make_group_x_qualifier(
    rsrcs: &[&Resource],
    prefix: &[String],
    group_by_fct: &[fn(&Resource) -> Option<String>],
    group_by_depth: usize,
    nodes: &mut Vec<TableNode>,
    used_mode: UsedMode,
) -> Vec<usize> {
    let mut out_indices = vec![];
    if let Some(group_by) = group_by_fct.get(group_by_depth) {
        for (key, group) in rsrcs
            .iter()
            .filter_map(|e| group_by(e).map(|k| (k, *e)))
            .into_group_map()
        {
            let mut key_full = prefix.to_vec();
            key_full.push(key.clone());
            let quantities = sum_by_qualifier(&group);
            let free = quantities.as_ref().and_then(|q| q.calc_free(used_mode));
            let idx = nodes.len();
            nodes.push(TableNode {
                key,
                path: key_full.clone(),
                quantities,
                free,
                children: vec![],
            });
            let child_indices = make_group_x_qualifier(
                &group,
                &key_full,
                group_by_fct,
                group_by_depth + 1,
                nodes,
                used_mode,
            );
            nodes[idx].children = child_indices;
            out_indices.push(idx);
        }
    }
    out_indices
}

fn accept_resource(name: &str, resource_filter: &[String]) -> bool {
    resource_filter.is_empty() || resource_filter.iter().any(|x| name.contains(x))
}

pub(crate) async fn refresh_kube_config(cli_opts: &CliOpts) -> Result<(), Error> {
    // force refresh token by calling "kubectl cluster-info before loading configuration"
    use std::process::Command;
    let mut cmd = Command::new("kubectl");
    cmd.arg("cluster-info");
    if let Some(ref kubeconfig) = cli_opts.kubeconfig {
        cmd.arg("--kubeconfig").arg(kubeconfig);
    }
    if let Some(ref context) = cli_opts.context {
        cmd.arg("--context").arg(context);
    }
    let output = cmd.output().map_err(|source| Error::CmdError {
        cmd: "kubectl cluster-info".to_owned(),
        output: None,
        source: Some(source),
    })?;
    if !output.status.success() {
        return Err(Error::CmdError {
            cmd: "kubectl cluster-info".to_owned(),
            output: Some(output),
            source: None,
        });
    }
    Ok(())
}

pub async fn new_client(cli_opts: &CliOpts) -> Result<kube::Client, Error> {
    if cli_opts.precheck {
        refresh_kube_config(cli_opts).await?;
    }
    let mut client_config = match (&cli_opts.kubeconfig, &cli_opts.context) {
        (Some(kubeconfig), context) => {
            let options = kube::config::KubeConfigOptions {
                context: context.clone(),
                ..Default::default()
            };
            kube::Config::from_custom_kubeconfig(
                kube::config::Kubeconfig::read_from(std::path::Path::new(kubeconfig)).map_err(
                    |source| Error::KubeConfigError {
                        context: format!("read kubeconfig from {}", kubeconfig.to_string_lossy()),
                        source,
                    },
                )?,
                &options,
            )
            .await
            .map_err(|source| Error::KubeConfigError {
                context: "create the kube client config from custom kubeconfig".to_string(),
                source,
            })?
        }
        (None, Some(context)) => kube::Config::from_kubeconfig(&kube::config::KubeConfigOptions {
            context: Some(context.clone()),
            ..Default::default()
        })
        .await
        .map_err(|source| Error::KubeConfigError {
            context: "create the kube client config".to_string(),
            source,
        })?,
        (None, None) => {
            kube::Config::infer()
                .await
                .map_err(|source| Error::KubeInferConfigError {
                    context: "create the kube client config".to_string(),
                    source,
                })?
        }
    };
    info!(cluster_url = client_config.cluster_url.to_string().as_str());
    if cli_opts.accept_invalid_certs {
        warn!(
            "TLS certificate verification is DISABLED (--accept-invalid-certs). This is insecure."
        );
    }
    client_config.accept_invalid_certs =
        client_config.accept_invalid_certs || cli_opts.accept_invalid_certs;
    kube::Client::try_from(client_config).map_err(|source| Error::KubeError {
        context: "create the kube client".to_string(),
        source,
    })
}

#[instrument]
pub async fn do_main(cli_opts: &CliOpts) -> Result<(), Error> {
    let client = new_client(cli_opts).await?;
    let mut resources: Vec<Resource> = vec![];
    let node_names = collect_from_nodes(
        client.clone(),
        &mut resources,
        &cli_opts.selector,
        &cli_opts.ignore_taints,
    )
    .await?;
    collect_from_pods(
        client.clone(),
        &mut resources,
        &cli_opts.namespace,
        &node_names,
    )
    .await?;

    let show_utilization = if cli_opts.utilization {
        match collect_from_metrics(client.clone(), &mut resources).await {
            Ok(_) => true,
            Err(err) => {
                warn!(?err);
                false
            }
        }
    } else {
        false
    };

    let group_by = cli_opts.effective_group_by();
    let sort_spec = parse_sort_spec(&cli_opts.sort)?;
    let effective_spec = effective_sort_spec(&sort_spec, show_utilization);
    let res = make_qualifiers(
        &resources,
        &group_by,
        &cli_opts.resource_name,
        &effective_spec,
        cli_opts.used_mode,
    );
    match &cli_opts.output {
        Output::Table => display_with_prettytable(&res, !&cli_opts.show_zero, show_utilization),
        Output::Csv => display_as_csv(&res, &group_by, show_utilization),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::should_include_node_by_taint;
    use k8s_openapi::api::core::v1::{Node, NodeSpec, Taint};

    fn qty(s: &str) -> Qty {
        s.parse().unwrap()
    }

    fn make_table_node(key: &str, requested: Option<&str>) -> TableNode {
        TableNode {
            key: key.to_string(),
            path: vec![key.to_string()],
            quantities: requested.map(|r| QtyByQualifier {
                requested: Some(qty(r)),
                ..Default::default()
            }),
            free: None,
            children: vec![],
        }
    }

    #[test]
    fn test_parse_sort_spec_full() {
        let spec = parse_sort_spec("usage DESC, requested DESC, limits DESC, name ASC").unwrap();
        assert_eq!(spec.len(), 4);
        assert_eq!(spec[0].column, SortColumnName::Usage);
        assert_eq!(spec[0].direction, SortDirection::Desc);
        assert_eq!(spec[1].column, SortColumnName::Requested);
        assert_eq!(spec[1].direction, SortDirection::Desc);
        assert_eq!(spec[2].column, SortColumnName::Limits);
        assert_eq!(spec[2].direction, SortDirection::Desc);
        assert_eq!(spec[3].column, SortColumnName::Name);
        assert_eq!(spec[3].direction, SortDirection::Asc);
    }

    #[test]
    fn test_parse_sort_spec_direction_optional() {
        let spec = parse_sort_spec("requested").unwrap();
        assert_eq!(spec.len(), 1);
        assert_eq!(spec[0].column, SortColumnName::Requested);
        assert_eq!(spec[0].direction, SortDirection::Asc);
    }

    #[test]
    fn test_parse_sort_spec_aliases() {
        let spec = parse_sort_spec("UTILIZATION asc, LIMIT DESC").unwrap();
        assert_eq!(spec[0].column, SortColumnName::Usage);
        assert_eq!(spec[0].direction, SortDirection::Asc);
        assert_eq!(spec[1].column, SortColumnName::Limits);
        assert_eq!(spec[1].direction, SortDirection::Desc);
    }

    #[test]
    fn test_parse_sort_spec_invalid() {
        let result = parse_sort_spec("invalid DESC");
        assert!(matches!(result, Err(Error::InvalidSortColumn { name }) if name == "invalid"));
    }

    #[test]
    fn test_effective_sort_spec_removes_usage() {
        let spec = parse_sort_spec("usage DESC, requested DESC").unwrap();
        let effective = effective_sort_spec(&spec, false);
        assert_eq!(effective.len(), 1);
        assert_eq!(effective[0].column, SortColumnName::Requested);
    }

    #[test]
    fn test_effective_sort_spec_keeps_usage_when_shown() {
        let spec = parse_sort_spec("usage DESC, requested DESC").unwrap();
        let effective = effective_sort_spec(&spec, true);
        assert_eq!(effective.len(), 2);
        assert_eq!(effective[0].column, SortColumnName::Usage);
    }

    #[test]
    fn test_sort_children_by_requested_desc() {
        let mut nodes = vec![
            make_table_node("node-a", Some("1000m")),
            make_table_node("node-b", Some("3000m")),
            make_table_node("node-c", Some("2000m")),
        ];
        let mut indices = vec![0usize, 1, 2];
        let spec = parse_sort_spec("requested DESC").unwrap();
        sort_children_recursive(&mut nodes, &mut indices, 1, 0, &spec);
        assert_eq!(indices, vec![1, 2, 0]); // 3000m, 2000m, 1000m
    }

    #[test]
    fn test_sort_children_none_is_infinity() {
        let mut nodes = vec![
            make_table_node("node-a", Some("1000m")),
            make_table_node("node-b", None), // None = infinity → first in DESC
            make_table_node("node-c", Some("2000m")),
        ];
        let mut indices = vec![0usize, 1, 2];
        let spec = parse_sort_spec("requested DESC").unwrap();
        sort_children_recursive(&mut nodes, &mut indices, 1, 0, &spec);
        assert_eq!(indices, vec![1, 2, 0]); // None first, then 2000m, 1000m
    }

    #[test]
    fn test_sort_children_name_asc_implicit_tiebreaker() {
        let mut nodes = vec![
            make_table_node("charlie", Some("1000m")),
            make_table_node("alice", Some("1000m")),
            make_table_node("bob", Some("1000m")),
        ];
        let mut indices = vec![0usize, 1, 2];
        let spec = parse_sort_spec("requested DESC").unwrap();
        sort_children_recursive(&mut nodes, &mut indices, 1, 0, &spec);
        // all requested equal → name ASC tiebreaker
        let names: Vec<&str> = indices.iter().map(|&i| nodes[i].key.as_str()).collect();
        assert_eq!(names, vec!["alice", "bob", "charlie"]);
    }

    #[test]
    fn test_sort_none_quantities_ancestor_level() {
        // Nodes with None quantities (e.g. namespace level) all tie → name ASC
        let mut nodes = vec![
            make_table_node("kube-system", None),
            make_table_node("default", None),
            make_table_node("monitoring", None),
        ];
        let mut indices = vec![0usize, 1, 2];
        let spec = parse_sort_spec("requested DESC, limits DESC").unwrap();
        sort_children_recursive(&mut nodes, &mut indices, 1, 0, &spec);
        let names: Vec<&str> = indices.iter().map(|&i| nodes[i].key.as_str()).collect();
        assert_eq!(names, vec!["default", "kube-system", "monitoring"]);
    }

    #[test]
    fn test_flatten_tree_dfs_order() {
        // root(0) → children [1, 2]; node 1 → children [3]
        let nodes = vec![
            TableNode {
                key: "root".into(),
                path: vec!["root".into()],
                quantities: None,
                free: None,
                children: vec![1, 2],
            },
            TableNode {
                key: "a".into(),
                path: vec!["root".into(), "a".into()],
                quantities: None,
                free: None,
                children: vec![3],
            },
            TableNode {
                key: "b".into(),
                path: vec!["root".into(), "b".into()],
                quantities: None,
                free: None,
                children: vec![],
            },
            TableNode {
                key: "a1".into(),
                path: vec!["root".into(), "a".into(), "a1".into()],
                quantities: None,
                free: None,
                children: vec![],
            },
        ];
        let flat = flatten_tree(&nodes, &[0]);
        let keys: Vec<&str> = flat
            .iter()
            .map(|(p, _, _)| p.last().unwrap().as_str())
            .collect();
        assert_eq!(keys, vec!["root", "a", "a1", "b"]);
    }

    #[test]
    fn test_resource_level_always_name_asc() {
        // At resource_depth, siblings sort by name ASC regardless of sort_spec
        let mut nodes = vec![
            make_table_node("memory", Some("8000000000")), // ~8Gi
            make_table_node("cpu", Some("3000m")),         // smaller i64 value
            make_table_node("pods", Some("110")),
        ];
        let mut indices = vec![0usize, 1, 2];
        let spec = parse_sort_spec("requested DESC").unwrap();
        // resource_depth = 0, depth = 0 → name ASC forced
        sort_children_recursive(&mut nodes, &mut indices, 0, 0, &spec);
        let names: Vec<&str> = indices.iter().map(|&i| nodes[i].key.as_str()).collect();
        assert_eq!(names, vec!["cpu", "memory", "pods"]);
    }

    #[test]
    fn test_non_resource_level_uses_sort_spec() {
        // At depth > resource_depth, sort spec applies
        let mut nodes = vec![
            make_table_node("node-a", Some("1000m")),
            make_table_node("node-b", Some("3000m")),
            make_table_node("node-c", Some("2000m")),
        ];
        let mut indices = vec![0usize, 1, 2];
        let spec = parse_sort_spec("requested DESC").unwrap();
        // resource_depth = 0, depth = 1 → sort spec applies
        sort_children_recursive(&mut nodes, &mut indices, 1, 0, &spec);
        let names: Vec<&str> = indices.iter().map(|&i| nodes[i].key.as_str()).collect();
        assert_eq!(names, vec!["node-b", "node-c", "node-a"]); // 3000m, 2000m, 1000m
    }

    fn create_test_node(name: &str, taints: Vec<Taint>) -> Node {
        Node {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: Some(name.to_string()),
                ..Default::default()
            },
            spec: Some(NodeSpec {
                taints: if taints.is_empty() {
                    None
                } else {
                    Some(taints)
                },
                ..Default::default()
            }),
            status: None,
        }
    }

    fn create_test_taint(key: &str, value: Option<&str>) -> Taint {
        Taint {
            key: key.to_string(),
            value: value.map(|s| s.to_string()),
            effect: "NoSchedule".to_string(),
            time_added: None,
        }
    }

    #[test]
    fn test_accept_resource() {
        assert!(accept_resource("cpu", &[]));
        assert!(accept_resource("cpu", &["c".to_string()]));
        assert!(accept_resource("cpu", &["cpu".to_string()]));
        assert!(!accept_resource("cpu", &["cpu3".to_string()]));
        assert!(accept_resource("gpu", &["gpu".to_string()]));
        assert!(accept_resource("nvidia.com/gpu", &["gpu".to_string()]));
    }

    #[test]
    fn test_should_include_node_by_taint_no_flag() {
        let node_without_taints = create_test_node("test-node", vec![]);
        let node_with_taints =
            create_test_node("test-node", vec![create_test_taint("key1", Some("value1"))]);

        assert!(should_include_node_by_taint(&node_without_taints, &None));
        assert!(!should_include_node_by_taint(&node_with_taints, &None));
    }

    #[test]
    fn test_should_include_node_by_taint_flag_without_values() {
        let node_without_taints = create_test_node("test-node", vec![]);
        let node_with_taints =
            create_test_node("test-node", vec![create_test_taint("key1", Some("value1"))]);

        assert!(should_include_node_by_taint(
            &node_without_taints,
            &Some(vec![])
        ));
        assert!(should_include_node_by_taint(
            &node_with_taints,
            &Some(vec![])
        ));
    }

    #[test]
    fn test_should_include_node_by_taint_specific_key() {
        let node_with_key1 =
            create_test_node("test-node", vec![create_test_taint("key1", Some("value1"))]);
        let node_with_key2 =
            create_test_node("test-node", vec![create_test_taint("key2", Some("value2"))]);
        let node_with_no_taints = create_test_node("test-node", vec![]);

        assert!(should_include_node_by_taint(
            &node_with_key1,
            &Some(vec!["key1".to_string()])
        ));
        assert!(!should_include_node_by_taint(
            &node_with_key2,
            &Some(vec!["key1".to_string()])
        ));
        assert!(should_include_node_by_taint(
            &node_with_no_taints,
            &Some(vec!["key1".to_string()])
        ));
    }

    #[test]
    fn test_should_include_node_by_taint_key_value_pair() {
        let node_with_matching_taint =
            create_test_node("test-node", vec![create_test_taint("key1", Some("value1"))]);
        let node_with_different_value =
            create_test_node("test-node", vec![create_test_taint("key1", Some("value2"))]);
        let node_with_different_key =
            create_test_node("test-node", vec![create_test_taint("key2", Some("value1"))]);
        let node_with_no_taints = create_test_node("test-node", vec![]);

        assert!(should_include_node_by_taint(
            &node_with_matching_taint,
            &Some(vec!["key1=value1".to_string()])
        ));
        assert!(!should_include_node_by_taint(
            &node_with_different_value,
            &Some(vec!["key1=value1".to_string()])
        ));
        assert!(!should_include_node_by_taint(
            &node_with_different_key,
            &Some(vec!["key1=value1".to_string()])
        ));
        assert!(should_include_node_by_taint(
            &node_with_no_taints,
            &Some(vec!["key1=value1".to_string()])
        ));
    }

    #[test]
    fn test_should_include_node_by_taint_multiple_patterns() {
        let node_with_key1 =
            create_test_node("test-node", vec![create_test_taint("key1", Some("value1"))]);
        let node_with_key2 =
            create_test_node("test-node", vec![create_test_taint("key2", Some("value2"))]);
        let node_with_both_keys = create_test_node(
            "test-node",
            vec![
                create_test_taint("key1", Some("value1")),
                create_test_taint("key2", Some("value2")),
            ],
        );
        let node_with_other_taint =
            create_test_node("test-node", vec![create_test_taint("key3", Some("value3"))]);
        let node_with_no_taints = create_test_node("test-node", vec![]);

        let patterns = vec!["key1".to_string(), "key2=value2".to_string()];
        assert!(should_include_node_by_taint(
            &node_with_key1,
            &Some(patterns.clone())
        ));
        assert!(should_include_node_by_taint(
            &node_with_key2,
            &Some(patterns.clone())
        ));
        assert!(should_include_node_by_taint(
            &node_with_both_keys,
            &Some(patterns.clone())
        ));
        assert!(!should_include_node_by_taint(
            &node_with_other_taint,
            &Some(patterns.clone())
        ));
        assert!(should_include_node_by_taint(
            &node_with_no_taints,
            &Some(patterns)
        ));
    }

    #[test]
    fn test_should_include_node_by_taint_real_world_examples() {
        let control_plane_node = create_test_node(
            "control-plane",
            vec![
                create_test_taint("node-role.kubernetes.io/control-plane", None),
                create_test_taint(
                    "node.kubernetes.io/exclude-from-external-load-balancers",
                    None,
                ),
            ],
        );

        let worker_node = create_test_node(
            "worker",
            vec![create_test_taint("dedicated", Some("database"))],
        );

        let untainted_node = create_test_node("untainted", vec![]);

        assert!(should_include_node_by_taint(
            &control_plane_node,
            &Some(vec!["node-role.kubernetes.io/control-plane".to_string()])
        ));
        assert!(!should_include_node_by_taint(
            &worker_node,
            &Some(vec!["node-role.kubernetes.io/control-plane".to_string()])
        ));
        assert!(should_include_node_by_taint(
            &untainted_node,
            &Some(vec!["node-role.kubernetes.io/control-plane".to_string()])
        ));

        assert!(should_include_node_by_taint(
            &worker_node,
            &Some(vec!["dedicated=database".to_string()])
        ));
        assert!(!should_include_node_by_taint(
            &control_plane_node,
            &Some(vec!["dedicated=database".to_string()])
        ));
        assert!(should_include_node_by_taint(
            &untainted_node,
            &Some(vec!["dedicated=database".to_string()])
        ));

        assert!(!should_include_node_by_taint(&control_plane_node, &None));
        assert!(!should_include_node_by_taint(&worker_node, &None));
        assert!(should_include_node_by_taint(&untainted_node, &None));

        assert!(should_include_node_by_taint(
            &control_plane_node,
            &Some(vec![])
        ));
        assert!(should_include_node_by_taint(&worker_node, &Some(vec![])));
        assert!(should_include_node_by_taint(&untainted_node, &Some(vec![])));
    }

    #[test]
    fn test_should_include_node_by_taint_edge_cases() {
        let node_with_key_only =
            create_test_node("test-node", vec![create_test_taint("key", None)]);

        let node_with_empty_key =
            create_test_node("test-node", vec![create_test_taint("", Some("value"))]);

        assert!(should_include_node_by_taint(
            &node_with_key_only,
            &Some(vec!["key".to_string()])
        ));

        let _result =
            should_include_node_by_taint(&node_with_empty_key, &Some(vec!["".to_string()]));
    }

    #[test]
    fn test_should_include_node_by_taint_any_taint_name() {
        let node_with_any_taint =
            create_test_node("test-node", vec![create_test_taint("any", Some("value"))]);

        let node_with_other_taint =
            create_test_node("test-node", vec![create_test_taint("other", Some("value"))]);

        let node_with_no_taints = create_test_node("test-node", vec![]);

        assert!(should_include_node_by_taint(
            &node_with_any_taint,
            &Some(vec!["any".to_string()])
        ));
        assert!(!should_include_node_by_taint(
            &node_with_other_taint,
            &Some(vec!["any".to_string()])
        ));
        assert!(should_include_node_by_taint(
            &node_with_no_taints,
            &Some(vec!["any".to_string()])
        ));

        assert!(!should_include_node_by_taint(&node_with_any_taint, &None));
        assert!(!should_include_node_by_taint(&node_with_other_taint, &None));
        assert!(should_include_node_by_taint(&node_with_no_taints, &None));

        assert!(should_include_node_by_taint(
            &node_with_any_taint,
            &Some(vec![])
        ));
        assert!(should_include_node_by_taint(
            &node_with_other_taint,
            &Some(vec![])
        ));
        assert!(should_include_node_by_taint(
            &node_with_no_taints,
            &Some(vec![])
        ));
    }
}
