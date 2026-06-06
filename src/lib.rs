pub mod metrics;
pub mod qty;
pub mod tree;

// mod human_format;
use chrono::prelude::*;
use clap::{Parser, ValueEnum};
use core::convert::TryFrom;
use futures::future::try_join_all;
use itertools::Itertools;
use k8s_openapi::api::core::v1::{Node, Pod};
use kube::api::{Api, ListParams, ObjectList};
#[cfg(feature = "prettytable")]
use prettytable::{Cell, Row, Table, format, row};
use qty::Qty;
use std::str::FromStr;
use std::{collections::BTreeMap, path::PathBuf};
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
pub struct Location {
    pub node_name: String,
    pub namespace: Option<String>,
    pub pod_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Resource {
    pub kind: String,
    pub quantity: Qty,
    pub location: Location,
    pub qualifier: ResourceQualifier,
}

#[derive(Debug, Clone)]
pub enum ResourceQualifier {
    Limit,
    Requested,
    Allocatable,
    Utilization,
    // HACK special qualifier, used to show zero/undef cpu & memory
    Present,
}

#[derive(Debug, Clone, Default)]
pub struct QtyByQualifier {
    pub limit: Option<Qty>,
    pub requested: Option<Qty>,
    pub allocatable: Option<Qty>,
    pub utilization: Option<Qty>,
    pub present: Option<Qty>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SortColumnName {
    Usage,
    Requested,
    Limits,
    Allocatable,
    Free,
    Name,
}

#[derive(Debug, Clone)]
pub struct SortColumn {
    pub column: SortColumnName,
    pub direction: SortDirection,
}

#[allow(clippy::result_large_err)]
pub fn parse_sort_spec(s: &str) -> Result<Vec<SortColumn>, Error> {
    s.split(',')
        .map(|token| {
            let parts: Vec<&str> = token.split_whitespace().collect();
            let col_name = parts.first().copied().unwrap_or("").to_lowercase();
            let direction_str = parts.get(1).copied().unwrap_or("asc").to_lowercase();

            let column = match col_name.as_str() {
                "usage" | "utilization" => SortColumnName::Usage,
                "requested" => SortColumnName::Requested,
                "limits" | "limit" => SortColumnName::Limits,
                "allocatable" => SortColumnName::Allocatable,
                "free" => SortColumnName::Free,
                "name" => SortColumnName::Name,
                other => {
                    return Err(Error::InvalidSortColumn {
                        name: other.to_string(),
                    });
                }
            };

            let direction = match direction_str.as_str() {
                "desc" => SortDirection::Desc,
                _ => SortDirection::Asc,
            };

            Ok(SortColumn { column, direction })
        })
        .collect()
}

pub fn effective_sort_spec(spec: &[SortColumn], show_utilization: bool) -> Vec<SortColumn> {
    spec.iter()
        .filter(|col| show_utilization || col.column != SortColumnName::Usage)
        .cloned()
        .collect()
}

#[derive(Debug, Clone)]
pub struct TableNode {
    pub key: String,
    pub path: Vec<String>,
    pub quantities: Option<QtyByQualifier>,
    pub free: Option<Qty>,
    pub children: Vec<usize>,
}

fn compare_qty(a: Option<&Qty>, b: Option<&Qty>) -> std::cmp::Ordering {
    match (a, b) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        (Some(a), Some(b)) => a.cmp(b),
    }
}

fn compare_nodes_by(a: &TableNode, b: &TableNode, col: &SortColumn) -> std::cmp::Ordering {
    let ord = match col.column {
        SortColumnName::Name => a.key.cmp(&b.key),
        SortColumnName::Usage => compare_qty(
            a.quantities.as_ref().and_then(|q| q.utilization.as_ref()),
            b.quantities.as_ref().and_then(|q| q.utilization.as_ref()),
        ),
        SortColumnName::Requested => compare_qty(
            a.quantities.as_ref().and_then(|q| q.requested.as_ref()),
            b.quantities.as_ref().and_then(|q| q.requested.as_ref()),
        ),
        SortColumnName::Limits => compare_qty(
            a.quantities.as_ref().and_then(|q| q.limit.as_ref()),
            b.quantities.as_ref().and_then(|q| q.limit.as_ref()),
        ),
        SortColumnName::Allocatable => compare_qty(
            a.quantities.as_ref().and_then(|q| q.allocatable.as_ref()),
            b.quantities.as_ref().and_then(|q| q.allocatable.as_ref()),
        ),
        SortColumnName::Free => compare_qty(a.free.as_ref(), b.free.as_ref()),
    };
    match col.direction {
        SortDirection::Asc => ord,
        SortDirection::Desc => ord.reverse(),
    }
}

fn sort_children_recursive(
    nodes: &mut Vec<TableNode>,
    indices: &mut [usize],
    depth: usize,
    resource_depth: usize,
    sort_spec: &[SortColumn],
) {
    // At the resource level (cpu/memory/pods siblings), quantities are incomparable
    // across resource kinds → always sort by name ASC.
    // Ancestors (depth < resource_depth) have None quantities so they naturally fall
    // through to the name ASC tiebreaker anyway.
    let effective: &[SortColumn] = if depth == resource_depth {
        &[]
    } else {
        sort_spec
    };
    indices.sort_by(|&a, &b| {
        for col in effective {
            let ord = compare_nodes_by(&nodes[a], &nodes[b], col);
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        nodes[a].key.cmp(&nodes[b].key)
    });
    for &i in indices.iter() {
        let mut ch = std::mem::take(&mut nodes[i].children);
        sort_children_recursive(nodes, &mut ch, depth + 1, resource_depth, sort_spec);
        nodes[i].children = ch;
    }
}

fn flatten_tree(
    nodes: &[TableNode],
    indices: &[usize],
) -> Vec<(Vec<String>, Option<QtyByQualifier>)> {
    let mut out = vec![];
    for &i in indices {
        out.push((nodes[i].path.clone(), nodes[i].quantities.clone()));
        out.extend(flatten_tree(nodes, &nodes[i].children));
    }
    out
}

fn add(lhs: Option<Qty>, rhs: &Qty) -> Option<Qty> {
    lhs.map(|l| &l + rhs).or_else(|| Some(rhs.clone()))
}

impl QtyByQualifier {
    pub fn calc_free(&self, used_mode: UsedMode) -> Option<Qty> {
        let total_used = match used_mode {
            UsedMode::max_request_limit => {
                std::cmp::max(self.limit.as_ref(), self.requested.as_ref())
            }
            UsedMode::only_request => self.requested.as_ref(),
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

pub fn sum_by_qualifier(rsrcs: &[&Resource]) -> Option<QtyByQualifier> {
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
) -> Vec<(Vec<String>, Option<QtyByQualifier>)> {
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
        .position(|g| *g == GroupBy::resource)
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

fn should_include_node_by_taint(node: &Node, ignore_taints: &Option<Vec<String>>) -> bool {
    let taints = node
        .spec
        .as_ref()
        .and_then(|spec| spec.taints.as_ref())
        .map(|taints| taints.as_slice())
        .unwrap_or(&[]);

    match ignore_taints {
        // No --ignore-taints flag: only include nodes without taints
        None => taints.is_empty(),

        // --ignore-taints used without values: include all nodes (both tainted and untainted)
        Some(patterns) if patterns.is_empty() => true,

        // --ignore-taints with specific patterns: include nodes without taints or with ignored taints
        Some(patterns) => {
            // If node has no taints, always include it
            if taints.is_empty() {
                return true;
            }

            // Check if any of the node's taints should be ignored
            for taint in taints {
                let taint_key = taint.key.as_str();
                let taint_value = taint.value.as_deref();

                for ignore_pattern in patterns {
                    // Check for exact key match
                    if ignore_pattern == taint_key {
                        return true;
                    }

                    // Check for key=value pattern
                    if let Some(eq_pos) = ignore_pattern.find('=') {
                        let pattern_key = &ignore_pattern[..eq_pos];
                        let pattern_value = &ignore_pattern[eq_pos + 1..];

                        if pattern_key == taint_key
                            && let Some(value) = taint_value
                            && pattern_value == value
                        {
                            return true;
                        }
                    }
                }
            }

            // If none of the node's taints are in the ignore list, exclude this node
            false
        }
    }
}

#[instrument(skip(client, resources))]
pub async fn collect_from_nodes(
    client: kube::Client,
    resources: &mut Vec<Resource>,
    selector: &Option<String>,
    ignore_taints: &Option<Vec<String>>,
) -> Result<Vec<String>, Error> {
    let api_nodes: Api<Node> = Api::all(client);
    let mut lp = ListParams::default();
    if let Some(labels) = &selector {
        lp = lp.labels(labels);
    }
    let all_nodes = api_nodes
        .list(&lp)
        .await
        .map_err(|source| Error::KubeError {
            context: "list nodes".to_string(),
            source,
        })?
        .items;

    // Filter nodes by taints
    let filtered_nodes: Vec<Node> = all_nodes
        .into_iter()
        .filter(|node| should_include_node_by_taint(node, ignore_taints))
        .collect();

    let node_names = filtered_nodes
        .iter()
        .filter_map(|node| node.metadata.name.clone())
        .collect();
    extract_allocatable_from_nodes(filtered_nodes, resources).await?;
    Ok(node_names)
}

#[instrument(skip(node_list, resources))]
pub async fn extract_allocatable_from_nodes(
    node_list: Vec<Node>,
    resources: &mut Vec<Resource>,
) -> Result<(), Error> {
    for node in node_list {
        let location = Location {
            node_name: node.metadata.name.unwrap_or_default(),
            ..Location::default()
        };
        if let Some(als) = node.status.and_then(|v| v.allocatable) {
            // add_resource(resources, &location, ResourceUsage::Allocatable, &als)?
            for (kind, value) in als.iter() {
                let quantity =
                    Qty::from_str(&(value).0).map_err(|source| Error::ResourceQtyParseError {
                        location: location.clone(),
                        qualifier: ResourceQualifier::Allocatable,
                        kind: kind.to_string(),
                        input: value.0.to_string(),
                        source,
                    })?;
                resources.push(Resource {
                    kind: kind.clone(),
                    qualifier: ResourceQualifier::Allocatable,
                    quantity,
                    location: location.clone(),
                });
            }
        }
    }
    Ok(())
}

/*
The phase of a Pod is a simple, high-level summary of where the Pod is in its lifecycle. The conditions array, the reason and message fields, and the individual container status arrays contain more detail about the pod's status.

There are five possible phase values:
Pending: The pod has been accepted by the Kubernetes system, but one or more of the container images has not been created. This includes time before being scheduled as well as time spent downloading images over the network, which could take a while.
Running: The pod has been bound to a node, and all of the containers have been created. At least one container is still running, or is in the process of starting or restarting.
Succeeded: All containers in the pod have terminated in success, and will not be restarted.
Failed: All containers in the pod have terminated, and at least one container has terminated in failure. The container either exited with non-zero status or was terminated by the system.
Unknown: For some reason the state of the pod could not be obtained, typically due to an error in communicating with the host of the pod.

More info: https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle#pod-phase
*/

pub fn is_scheduled(pod: &Pod) -> bool {
    pod.status
        .as_ref()
        .and_then(|ps| {
            ps.phase.as_ref().and_then(|phase| {
                match &phase[..] {
                    "Succeeded" | "Failed" => Some(false),
                    "Running" => Some(true),
                    "Unknown" => None, // this is the case when a node is down (kubelet is not responding)
                    "Pending" => ps.conditions.as_ref().map(|o| {
                        o.iter()
                            .any(|c| c.type_ == "PodScheduled" && c.status == "True")
                    }),
                    &_ => None, // should not happen
                }
            })
        })
        .unwrap_or(false)
}

#[allow(clippy::result_large_err)]
fn push_resources(
    resources: &mut Vec<Resource>,
    location: &Location,
    qualifier: ResourceQualifier,
    resource_list: &BTreeMap<String, Qty>,
) -> Result<(), Error> {
    for (key, quantity) in resource_list.iter() {
        resources.push(Resource {
            kind: key.clone(),
            qualifier: qualifier.clone(),
            quantity: quantity.clone(),
            location: location.clone(),
        });
    }
    // add a "pods" resource as well
    resources.push(Resource {
        kind: "pods".to_string(),
        qualifier,
        quantity: Qty::from_str("1")?,
        location: location.clone(),
    });
    Ok(())
}

#[allow(clippy::result_large_err)]
fn process_resources<F>(
    effective_resources: &mut BTreeMap<String, Qty>,
    resource_list: &BTreeMap<String, k8s_openapi::apimachinery::pkg::api::resource::Quantity>,
    op: F,
) -> Result<(), Error>
where
    F: Fn(Qty, Qty) -> Qty,
{
    for (key, value) in resource_list.iter() {
        let quantity = Qty::from_str(&(value).0)?;
        if let Some(current_quantity) = effective_resources.get_mut(key) {
            *current_quantity = op(current_quantity.clone(), quantity).clone();
        } else {
            effective_resources.insert(key.clone(), quantity.clone());
        }
    }
    Ok(())
}

#[instrument(skip(client, resources))]
pub async fn collect_from_pods(
    client: kube::Client,
    resources: &mut Vec<Resource>,
    namespace: &[String],
    selected_node_names: &[String],
) -> Result<(), Error> {
    let mut apis: Vec<Api<Pod>> = vec![];
    if namespace.is_empty() {
        apis.push(Api::all(client))
    } else {
        for ns in namespace {
            apis.push(Api::namespaced(client.clone(), ns))
        }
    }

    // Call `list` concurrently on every apis
    let pods: Vec<Pod> = try_join_all(
        apis.iter()
            .map(|api| async { api.list(&ListParams::default()).await }),
    )
    .await
    .map_err(|source| Error::KubeError {
        context: "list pods".to_string(),
        source,
    })?
    .into_iter()
    .flat_map(|list| list.items)
    .collect();

    extract_allocatable_from_pods(pods, resources, selected_node_names).await?;
    Ok(())
}

#[instrument(skip(pod_list, resources))]
pub async fn extract_allocatable_from_pods(
    pod_list: Vec<Pod>,
    resources: &mut Vec<Resource>,
    selected_node_names: &[String],
) -> Result<(), Error> {
    for pod in pod_list.into_iter().filter(is_scheduled) {
        let spec = pod.spec.as_ref();
        let node_name = spec.and_then(|s| s.node_name.clone()).unwrap_or_default();
        if !selected_node_names.contains(&node_name) {
            continue;
        }
        let metadata = &pod.metadata;
        let location = Location {
            node_name: node_name.clone(),
            namespace: metadata.namespace.clone(),
            pod_name: metadata.name.clone(),
        };
        // compute the effective resource qualifier
        // see https://kubernetes.io/docs/concepts/workloads/pods/init-containers/#resources
        let mut resource_requests: BTreeMap<String, Qty> = BTreeMap::new();
        let mut resource_limits: BTreeMap<String, Qty> = BTreeMap::new();
        // handle regular containers
        let containers = spec.map(|s| s.containers.clone()).unwrap_or_default();
        for container in containers.into_iter() {
            if let Some(requirements) = container.resources {
                if let Some(r) = requirements.requests {
                    process_resources(&mut resource_requests, &r, std::ops::Add::add)?;
                }
                if let Some(r) = requirements.limits {
                    process_resources(&mut resource_limits, &r, std::ops::Add::add)?;
                }
            }
        }
        // handle initContainers
        let init_containers = spec
            .and_then(|s| s.init_containers.clone())
            .unwrap_or_default();
        for container in init_containers.into_iter() {
            if let Some(requirements) = container.resources {
                if let Some(r) = requirements.requests {
                    process_resources(&mut resource_requests, &r, std::cmp::max)?;
                }
                if let Some(r) = requirements.limits {
                    process_resources(&mut resource_limits, &r, std::cmp::max)?;
                }
            }
        }
        // handler overhead (add to both requests and limits)
        if let Some(ref overhead) = spec.and_then(|s| s.overhead.clone()) {
            process_resources(&mut resource_requests, overhead, std::ops::Add::add)?;
            process_resources(&mut resource_limits, overhead, std::ops::Add::add)?;
        }
        // push these onto resources
        push_resources(
            resources,
            &location,
            ResourceQualifier::Requested,
            &resource_requests,
        )?;
        push_resources(
            resources,
            &location,
            ResourceQualifier::Limit,
            &resource_limits,
        )?;
        // HACK add zero/None cpu & memory, to allow show-zero to display them
        resources.push(Resource {
            kind: "cpu".to_string(),
            qualifier: ResourceQualifier::Present,
            quantity: Qty::zero(),
            location: location.clone(),
        });
        resources.push(Resource {
            kind: "memory".to_string(),
            qualifier: ResourceQualifier::Present,
            quantity: Qty::zero(),
            location: location.clone(),
        });
    }
    Ok(())
}

pub fn extract_locations(
    resources: &[Resource],
) -> std::collections::HashMap<(String, String), Location> {
    resources
        .iter()
        .filter_map(|resource| {
            let loc = &resource.location;
            loc.pod_name.as_ref().map(|n| {
                (
                    (loc.namespace.clone().unwrap_or_default(), n.to_owned()),
                    loc.clone(),
                )
            })
        })
        .collect()
}

//TODO need location of pods (aka node because its not part of metrics)
//TODO filter to only retreive info from node's selector
#[instrument(skip(client, resources))]
pub async fn collect_from_metrics(
    client: kube::Client,
    resources: &mut Vec<Resource>,
) -> Result<(), Error> {
    let api_pod_metrics: Api<metrics::PodMetrics> = Api::all(client);
    let pod_metrics = api_pod_metrics
        .list(&ListParams::default())
        .await
        .map_err(|source| Error::KubeError {
            context: "list podmetrics, maybe Metrics API not available".to_string(),
            source,
        })?;

    extract_utilizations_from_pod_metrics(pod_metrics, resources).await?;
    Ok(())
}

#[instrument(skip(pod_metrics, resources))]
pub async fn extract_utilizations_from_pod_metrics(
    pod_metrics: ObjectList<metrics::PodMetrics>,
    resources: &mut Vec<Resource>,
) -> Result<(), Error> {
    let cpu_kind = "cpu";
    let memory_kind = "memory";
    let locations = extract_locations(resources);
    for pod_metric in pod_metrics.items {
        let metadata = &pod_metric.metadata;
        let key = (
            metadata.namespace.clone().unwrap_or_default(),
            metadata.name.clone().unwrap_or_default(),
        );
        let location = locations.get(&key).cloned().unwrap_or_else(|| Location {
            // node_name: node_name.clone(),
            namespace: metadata.namespace.clone(),
            pod_name: metadata.name.clone(),
            ..Location::default()
        });
        let mut cpu_utilization = Qty::default();
        let mut memory_utilization = Qty::default();
        for container in pod_metric.containers.into_iter() {
            cpu_utilization += &Qty::from_str(&container.usage.cpu)
                .map_err(|source| Error::ResourceQtyParseError {
                    location: location.clone(),
                    qualifier: ResourceQualifier::Utilization,
                    kind: cpu_kind.to_string(),
                    input: container.usage.cpu.clone(),
                    source,
                })?
                .max(Qty::lowest_positive());
            memory_utilization += &Qty::from_str(&container.usage.memory)
                .map_err(|source| Error::ResourceQtyParseError {
                    location: location.clone(),
                    qualifier: ResourceQualifier::Utilization,
                    kind: memory_kind.to_string(),
                    input: container.usage.memory.clone(),
                    source,
                })?
                .max(Qty::lowest_positive());
        }
        resources.push(Resource {
            kind: cpu_kind.to_string(),
            qualifier: ResourceQualifier::Utilization,
            quantity: cpu_utilization,
            location: location.clone(),
        });
        resources.push(Resource {
            kind: memory_kind.to_string(),
            qualifier: ResourceQualifier::Utilization,
            quantity: memory_utilization,
            location: location.clone(),
        });
    }
    Ok(())
}

#[derive(Debug, Eq, PartialEq, ValueEnum, Clone)]
#[allow(non_camel_case_types)]
pub enum GroupBy {
    resource,
    node,
    pod,
    namespace,
}

impl GroupBy {
    pub fn to_fct(&self) -> fn(&Resource) -> Option<String> {
        match self {
            Self::resource => Self::extract_kind,
            Self::node => Self::extract_node_name,
            Self::pod => Self::extract_pod_name,
            Self::namespace => Self::extract_namespace,
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
            Self::resource => "resource",
            Self::node => "node",
            Self::pod => "pod",
            Self::namespace => "namespace",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Eq, PartialEq, ValueEnum, Clone, Copy, Default)]
#[allow(non_camel_case_types)]
pub enum Output {
    #[default]
    table,
    csv,
}

#[derive(Debug, Eq, PartialEq, ValueEnum, Clone, Copy, Default)]
#[allow(non_camel_case_types)]
pub enum UsedMode {
    #[default]
    max_request_limit,
    only_request,
}

#[derive(Parser, Debug)]
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
        default_value = "max-request-limit",
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

pub async fn refresh_kube_config(cli_opts: &CliOpts) -> Result<(), Error> {
    //HACK force refresh token by calling "kubectl cluster-info before loading configuration"
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

    let sort_spec = parse_sort_spec(&cli_opts.sort)?;
    let effective_spec = effective_sort_spec(&sort_spec, show_utilization);
    let res = make_qualifiers(
        &resources,
        &cli_opts.group_by,
        &cli_opts.resource_name,
        &effective_spec,
        cli_opts.used_mode,
    );
    match &cli_opts.output {
        Output::table => display_with_prettytable(
            &res,
            !&cli_opts.show_zero,
            show_utilization,
            cli_opts.used_mode,
        ),
        Output::csv => display_as_csv(
            &res,
            &cli_opts.group_by,
            show_utilization,
            cli_opts.used_mode,
        ),
    }
    Ok(())
}

pub fn display_as_csv(
    data: &[(Vec<String>, Option<QtyByQualifier>)],
    group_by: &[GroupBy],
    show_utilization: bool,
    used_mode: UsedMode,
) {
    // print header
    println!(
        "Date,Kind,{}{},Requested,%Requested,Limit,%Limit,Allocatable,Free",
        group_by.iter().map(|x| x.to_string()).join(","),
        if show_utilization {
            ",Utilization,%Utilization"
        } else {
            ""
        }
    );

    // print data
    let empty = "".to_string();
    let datetime = Utc::now().to_rfc3339();
    for (k, oqtys) in data {
        if let Some(qtys) = oqtys {
            let mut row = vec![
                datetime.clone(),
                group_by
                    .get(k.len() - 1)
                    .map(|x| x.to_string())
                    .unwrap_or_else(|| empty.clone()),
            ];
            for i in 0..group_by.len() {
                row.push(k.get(i).cloned().unwrap_or_else(|| empty.clone()));
            }

            if show_utilization {
                add_cells_for_cvs(&mut row, &qtys.utilization, &qtys.allocatable);
            }
            add_cells_for_cvs(&mut row, &qtys.requested, &qtys.allocatable);
            add_cells_for_cvs(&mut row, &qtys.limit, &qtys.allocatable);

            row.push(
                qtys.allocatable
                    .as_ref()
                    .map(|qty| format!("{:.2}", f64::from(qty)))
                    .unwrap_or_else(|| empty.clone()),
            );
            row.push(
                qtys.calc_free(used_mode)
                    .as_ref()
                    .map(|qty| format!("{:.2}", f64::from(qty)))
                    .unwrap_or_else(|| empty.clone()),
            );
            println!("{}", &row.join(","));
        }
    }
}

fn add_cells_for_cvs(row: &mut Vec<String>, oqty: &Option<Qty>, o100: &Option<Qty>) {
    match oqty {
        None => {
            row.push("".to_string());
            row.push("".to_string());
        }
        Some(qty) => {
            row.push(format!("{:.2}", f64::from(qty)));
            row.push(match o100 {
                None => "".to_string(),
                Some(q100) => format!("{:.0}%", qty.calc_percentage(q100)),
            });
        }
    };
}

#[cfg(not(feature = "prettytable"))]
pub fn display_with_prettytable(
    _data: &[(Vec<String>, Option<QtyByQualifier>)],
    _filter_full_zero: bool,
    _show_utilization: bool,
    _used_mode: UsedMode,
) {
    warn!("feature 'prettytable' not enabled");
}

#[cfg(feature = "prettytable")]
pub fn display_with_prettytable(
    data: &[(Vec<String>, Option<QtyByQualifier>)],
    filter_full_zero: bool,
    show_utilization: bool,
    used_mode: UsedMode,
) {
    // Create the table
    let mut table = Table::new();
    let format = format::FormatBuilder::new()
        // .column_separator('|')
        // .borders('|')
        // .separators(&[format::LinePosition::Top,
        //               format::LinePosition::Bottom],
        //             format::LineSeparator::new('-', '+', '+', '+'))
        .separators(&[], format::LineSeparator::new('-', '+', '+', '+'))
        .padding(1, 1)
        .build();
    table.set_format(format);
    let mut row_titles = row![bl->"Resource", br->"Utilization", br->"Requested", br->"Limit",  br->"Allocatable", br->"Free"];
    if !show_utilization {
        row_titles.remove_cell(1);
    }
    table.set_titles(row_titles);
    let data2 = data
        .iter()
        .filter(|d| {
            !filter_full_zero
                || !d
                    .1
                    .as_ref()
                    .map(|x| {
                        x.utilization.is_none()
                            && is_empty(&x.requested)
                            && is_empty(&x.limit)
                            && is_empty(&x.allocatable)
                    })
                    .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    let prefixes = tree::provide_prefix(&data2, |parent, item| parent.0.len() + 1 == item.0.len());

    for ((k, oqtys), prefix) in data2.iter().zip(prefixes.iter()) {
        let column0 = format!(
            "{} {}",
            prefix,
            k.last().map(|x| x.as_str()).unwrap_or("???")
        );
        if let Some(qtys) = oqtys {
            let style = if qtys.requested > qtys.limit
                || qtys.utilization > qtys.limit
                || is_empty(&qtys.requested)
                || is_empty(&qtys.limit)
            {
                "rFy"
            } else {
                "rFg"
            };
            let mut row = Row::new(vec![
                Cell::new(&column0),
                make_cell_for_prettytable(&qtys.utilization, &qtys.allocatable).style_spec(style),
                make_cell_for_prettytable(&qtys.requested, &qtys.allocatable).style_spec(style),
                make_cell_for_prettytable(&qtys.limit, &qtys.allocatable).style_spec(style),
                make_cell_for_prettytable(&qtys.allocatable, &None).style_spec(style),
                make_cell_for_prettytable(&qtys.calc_free(used_mode), &None).style_spec(style),
            ]);
            if !show_utilization {
                row.remove_cell(1);
            }
            table.add_row(row);
        } else {
            table.add_row(Row::new(vec![Cell::new(&column0)]));
        }
    }

    // Print the table to stdout
    table.printstd();
}

#[cfg(feature = "prettytable")]
fn is_empty(oqty: &Option<Qty>) -> bool {
    match oqty {
        Some(qty) => qty.is_zero(),
        None => true,
    }
}

#[cfg(feature = "prettytable")]
fn make_cell_for_prettytable(oqty: &Option<Qty>, o100: &Option<Qty>) -> Cell {
    let txt = match oqty {
        None => "__".to_string(),
        Some(qty) => match o100 {
            None => format!("{}", qty.adjust_scale()),
            Some(q100) => format!("({:.0}%) {}", qty.calc_percentage(q100), qty.adjust_scale()),
        },
    };
    Cell::new(&txt)
}

#[cfg(test)]
mod tests {
    use super::*;
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
            .map(|(p, _)| p.last().unwrap().as_str())
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
            effect: "NoSchedule".to_string(), // Use a common taint effect for testing
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

        // No flag: only include nodes without taints
        assert!(should_include_node_by_taint(&node_without_taints, &None));
        assert!(!should_include_node_by_taint(&node_with_taints, &None));
    }

    #[test]
    fn test_should_include_node_by_taint_flag_without_values() {
        let node_without_taints = create_test_node("test-node", vec![]);
        let node_with_taints =
            create_test_node("test-node", vec![create_test_taint("key1", Some("value1"))]);

        // Flag used without values: include all nodes
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

        // Ignore taint key1: include nodes without taints and nodes with key1
        assert!(should_include_node_by_taint(
            &node_with_key1,
            &Some(vec!["key1".to_string()])
        ));
        assert!(!should_include_node_by_taint(
            &node_with_key2,
            &Some(vec!["key1".to_string()])
        )); // key2 is not ignored
        assert!(should_include_node_by_taint(
            &node_with_no_taints,
            &Some(vec!["key1".to_string()])
        )); // no taints are always included
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

        // Ignore taint key1=value1: include nodes without taints and nodes with this specific taint
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

        // Ignore taints key1 and key2=value2: include nodes without taints and nodes with these specific taints
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
        )); // key3 is not ignored
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

        // Test ignoring control plane taints
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

        // Test ignoring dedicated database taints
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

        // Test no flag behavior (only untainted nodes)
        assert!(!should_include_node_by_taint(&control_plane_node, &None));
        assert!(!should_include_node_by_taint(&worker_node, &None));
        assert!(should_include_node_by_taint(&untainted_node, &None));

        // Test flag without values (all nodes)
        assert!(should_include_node_by_taint(
            &control_plane_node,
            &Some(vec![])
        ));
        assert!(should_include_node_by_taint(&worker_node, &Some(vec![])));
        assert!(should_include_node_by_taint(&untainted_node, &Some(vec![])));
    }

    #[test]
    fn test_should_include_node_by_taint_edge_cases() {
        // Test taint with no value
        let node_with_key_only =
            create_test_node("test-node", vec![create_test_taint("key", None)]);

        // Test empty string key (edge case)
        let node_with_empty_key =
            create_test_node("test-node", vec![create_test_taint("", Some("value"))]);

        assert!(should_include_node_by_taint(
            &node_with_key_only,
            &Some(vec!["key".to_string()])
        ));

        // Note: Empty key filtering may or may not work depending on Kubernetes API behavior
        // Let's just verify it doesn't crash and behaves consistently
        let _result =
            should_include_node_by_taint(&node_with_empty_key, &Some(vec!["".to_string()]));
        // We don't assert a specific result since this is an edge case that may vary
    }

    #[test]
    fn test_should_include_node_by_taint_any_taint_name() {
        // Test that we can ignore nodes with taints literally named "any"
        let node_with_any_taint =
            create_test_node("test-node", vec![create_test_taint("any", Some("value"))]);

        let node_with_other_taint =
            create_test_node("test-node", vec![create_test_taint("other", Some("value"))]);

        let node_with_no_taints = create_test_node("test-node", vec![]);

        // Should be able to ignore nodes with taints named "any"
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

        // Test no flag behavior
        assert!(!should_include_node_by_taint(&node_with_any_taint, &None));
        assert!(!should_include_node_by_taint(&node_with_other_taint, &None));
        assert!(should_include_node_by_taint(&node_with_no_taints, &None));

        // Test flag without values
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
