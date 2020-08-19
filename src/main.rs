mod qty;
mod tree;
// mod human_format;
use anyhow::{anyhow, Context, Result};
use env_logger;
use itertools::Itertools;
use log::error;
use qty::Qty;
use std::str::FromStr;
use structopt::clap::arg_enum;
use structopt::clap::AppSettings;
use structopt::StructOpt;

use k8s_openapi::api::core::v1::{Node, Pod};
use kube::api::{Api, ListParams};

#[derive(Debug, Clone, Default)]
struct Location {
    node_name: Option<String>,
    namespace: Option<String>,
    pod_name: Option<String>,
    container_name: Option<String>,
}

#[derive(Debug, Clone)]
struct Resource {
    kind: String,
    quantity: Qty,
    location: Location,
    usage: ResourceUsage,
}

#[derive(Debug, Clone)]
enum ResourceUsage {
    Limit,
    Requested,
    Allocatable,
}

#[derive(Debug, Clone, Default)]
struct QtyOfUsage {
    limit: Qty,
    requested: Qty,
    allocatable: Qty,
}

impl QtyOfUsage {
    pub fn calc_free(&self) -> Qty {
        let total_used = if self.limit > self.requested {
            &self.limit
        } else {
            &self.requested
        };
        if self.allocatable > *total_used {
            &self.allocatable - total_used
        } else {
            Qty::default()
        }
    }
}

fn sum_by_usage(rsrcs: &[&Resource]) -> Option<QtyOfUsage> {
    if !rsrcs.is_empty() {
        let kind = rsrcs
            .get(0)
            .expect("group contains at least 1 element")
            .kind
            .clone();

        if rsrcs.iter().all(|i| i.kind == kind) {
            let sum = rsrcs.iter().fold(QtyOfUsage::default(), |mut acc, v| {
                match &v.usage {
                    ResourceUsage::Limit => acc.limit += &v.quantity,
                    ResourceUsage::Requested => acc.requested += &v.quantity,
                    ResourceUsage::Allocatable => acc.allocatable += &v.quantity,
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

fn make_usages(rsrcs: &[Resource], group_by: &[GroupBy]) -> Vec<(Vec<String>, Option<QtyOfUsage>)> {
    let group_by_fct = group_by.iter().map(GroupBy::to_fct).collect::<Vec<_>>();
    let mut out = make_group_x_usage(&(rsrcs.iter().collect::<Vec<_>>()), &[], &group_by_fct, 0);
    out.sort_by_key(|i| i.0.clone());
    out
}

fn make_group_x_usage(
    rsrcs: &[&Resource],
    prefix: &[String],
    group_by_fct: &[fn(&Resource) -> Option<String>],
    group_by_depth: usize,
) -> Vec<(Vec<String>, Option<QtyOfUsage>)> {
    // Note: The `&` is significant here, `GroupBy` is iterable
    // only by reference. You can also call `.into_iter()` explicitly.
    let mut out = vec![];
    if let Some(group_by) = group_by_fct.get(group_by_depth) {
        for (key, group) in rsrcs
            .iter()
            .filter_map(|e| group_by(e).map(|k| (k, *e)))
            .into_group_map()
        {
            let mut key_full = prefix.to_vec();
            key_full.push(key);
            let children = make_group_x_usage(&group, &key_full, group_by_fct, group_by_depth + 1);
            out.push((key_full, sum_by_usage(&group)));
            out.extend(children);
        }
    }
    // let kg = &rsrcs.into_iter().group_by(|v| v.kind);
    // kg.into_iter().map(|(key, group)|  ).collect()
    out
}

fn accept_resource(name: &str, resource_filter: &[String]) -> bool {
    resource_filter.is_empty() || resource_filter.iter().any(|x| name.contains(x))
}

async fn collect_from_nodes(
    client: kube::Client,
    resources: &mut Vec<Resource>,
    resource_names: &[String],
) -> Result<()> {
    let api_nodes: Api<Node> = Api::all(client);
    let nodes = api_nodes
        .list(&ListParams::default())
        .await
        .with_context(|| "Failed to list nodes via k8s api".to_string())?;
    for node in nodes.items {
        let location = Location {
            node_name: node.metadata.name,
            ..Location::default()
        };
        if let Some(als) = node.status.and_then(|v| v.allocatable) {
            for a in als
                .into_iter()
                .filter(|a| accept_resource(&a.0, resource_names))
            {
                let quantity = Qty::from_str(&(a.1).0).with_context(|| {
                    format!(
                        "Failed to read Qty of location {:?} / available {:?}",
                        &location, &a
                    )
                })?;
                resources.push(Resource {
                    kind: a.0,
                    usage: ResourceUsage::Allocatable,
                    quantity,
                    location: location.clone(),
                });
            }
        }
    }
    Ok(())
}

async fn collect_from_pods(
    client: kube::Client,
    resources: &mut Vec<Resource>,
    resource_names: &[String],
    namespace: &Option<String>,
) -> Result<()> {
    let api_pods: Api<Pod> = if let Some(ns) = namespace {
        Api::namespaced(client, &ns)
    } else {
        Api::all(client)
    };
    let pods = api_pods
        .list(&ListParams::default())
        .await
        .with_context(|| "Failed to list pods via k8s api".to_string())?;
    for pod in pods.items.into_iter().filter(|pod| {
        pod.status
            .as_ref()
            .and_then(|ps| ps.phase.as_ref().map(|s| s == "Running"))
            .unwrap_or(false)
    }) {
        let status = pod.status.as_ref();
        let spec = pod.spec.as_ref();
        let node_name = status
            .and_then(|v| v.nominated_node_name.clone())
            .or_else(|| spec.and_then(|m| m.node_name.clone()));
        let containers = spec.map(|s| s.containers.clone()).unwrap_or_else(|| vec![]);
        for (_, container) in containers.into_iter().enumerate().filter(|(i, _)| {
            status
                .and_then(|s| s.container_statuses.as_ref())
                .and_then(|v| v.get(*i).and_then(|v| v.state.as_ref()))
                .map(|s| s.running.is_some())
                .unwrap_or(true)
        }) {
            let metadata = &pod.metadata;
            let location = Location {
                node_name: node_name.clone(),
                namespace: metadata.namespace.clone(),
                pod_name: metadata.name.clone(),
                container_name: Some(container.name.clone()),
            };
            if let Some(requirements) = container.resources {
                if let Some(r) = requirements.requests {
                    for request in r
                        .into_iter()
                        .filter(|a| accept_resource(&a.0, resource_names))
                    {
                        let quantity = Qty::from_str(&(request.1).0).with_context(|| {
                            format!(
                                "Failed to read Qty of location {:?} / request {:?}",
                                &location, &request
                            )
                        })?;
                        resources.push(Resource {
                            kind: request.0,
                            usage: ResourceUsage::Requested,
                            quantity,
                            location: location.clone(),
                        });
                    }
                }
                if let Some(l) = requirements.limits {
                    for limit in l
                        .into_iter()
                        .filter(|a| accept_resource(&a.0, resource_names))
                    {
                        let quantity = Qty::from_str(&(limit.1).0).with_context(|| {
                            format!(
                                "Failed to read Qty of location {:?} / limit {:?}",
                                &location, &limit
                            )
                        })?;
                        resources.push(Resource {
                            kind: limit.0,
                            usage: ResourceUsage::Limit,
                            quantity,
                            location: location.clone(),
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

arg_enum! {
    #[derive(Debug, Eq, PartialEq)]
    #[allow(non_camel_case_types)]
    enum GroupBy {
        resource,
        node,
        pod,
        namespace,
    }
}

impl GroupBy {
    fn to_fct(&self) -> fn(&Resource) -> Option<String> {
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
        e.location.node_name.clone()
    }

    fn extract_pod_name(e: &Resource) -> Option<String> {
        e.location.pod_name.clone()
    }

    fn extract_namespace(e: &Resource) -> Option<String> {
        e.location.namespace.clone()
    }
}

arg_enum! {
    #[derive(Debug, Eq, PartialEq)]
    #[allow(non_camel_case_types)]
    enum Output {
        table,
        csv,
    }
}

#[derive(StructOpt, Debug)]
#[structopt(
    global_settings(&[AppSettings::ColoredHelp, AppSettings::VersionlessSubcommands]),
    author = env!("CARGO_PKG_HOMEPAGE"), about
)]
struct CliOpts {
    /// Show only pods from this namespace
    #[structopt(short, long)]
    namespace: Option<String>,

    /// Show lines with zero requested and zero limit and zero allocatable
    #[structopt(short = "z", long)]
    show_zero: bool,

    /// Filter resources shown by name(s), by default all resources are listed
    #[structopt(short, long)]
    resource_name: Vec<String>,

    /// Group informations hierarchically (default: -g resource -g node -g pod)
    #[structopt(short, long, possible_values = &GroupBy::variants(), case_insensitive = true)]
    group_by: Vec<GroupBy>,

    /// Output format
    #[structopt(short, long, possible_values = &Output::variants(), case_insensitive = true, default_value = "table")]
    output: Output,
}

#[tokio::main]
async fn main() -> () {
    // std::env::set_var("RUST_LOG", "info,kube=trace");
    env_logger::init();
    let mut cli_opts = CliOpts::from_args();
    //HACK because I didn't find how to default a multiple opts
    if cli_opts.group_by.is_empty() {
        cli_opts.group_by.push(GroupBy::resource);
        cli_opts.group_by.push(GroupBy::node);
        cli_opts.group_by.push(GroupBy::pod);
    }
    if !cli_opts.group_by.contains(&GroupBy::resource) {
        cli_opts.group_by.insert(0, GroupBy::resource)
    }
    cli_opts.group_by.dedup();
    // dbg!(&cli_opts);

    let r = do_main(&cli_opts).await;
    if let Err(e) = r {
        error!("failed \ncli: {:?}\nerror: {:?}", &cli_opts, &e);
    }
}

async fn refresh_kube_config() -> Result<()> {
    //HACK force refresh token by calling "kubectl cluster-info before loading configuration"
    use std::process::Command;
    let output = Command::new("kubectl")
        .arg("cluster-info")
        .output()
        .with_context(|| "failed to executed 'kubectl cluster-info'")?;
    if !output.status.success() {
        return Err(anyhow!("`kubectl cluster-info` failed with: {:?}", &output));
    }
    Ok(())
}

async fn do_main(cli_opts: &CliOpts) -> Result<()> {
    refresh_kube_config()
        .await
        .with_context(|| "failed to refresh kubectl config".to_string())?;
    let client = kube::Client::try_default().await?;

    let mut resources: Vec<Resource> = vec![];
    collect_from_nodes(client.clone(), &mut resources, &cli_opts.resource_name)
        .await
        .with_context(|| "failed to collect info from nodes".to_string())?;
    collect_from_pods(
        client.clone(),
        &mut resources,
        &cli_opts.resource_name,
        &cli_opts.namespace,
    )
    .await
    .with_context(|| "failed to collect info from pods".to_string())?;

    let res = make_usages(&resources, &cli_opts.group_by);
    match &cli_opts.output {
        Output::table => display_with_prettytable(&res, !&cli_opts.show_zero),
        Output::csv => display_as_csv(&res, &cli_opts.group_by),
    }
    Ok(())
}
fn display_as_csv(data: &[(Vec<String>, Option<QtyOfUsage>)], group_by: &[GroupBy]) {
    // print header
    println!(
        "{},Requested,%Requested,Limit,%Limit,Allocatable,Free",
        group_by.iter().map(|x| x.to_string()).join(",")
    );

    // print data
    let empty = "".to_string();
    for (k, oqtys) in data {
        let mut row = vec![];
        for i in 0..group_by.len() {
            row.push(k.get(i).cloned().unwrap_or_else(|| empty.clone()));
        }
        if let Some(qtys) = oqtys {
            if qtys.allocatable.is_zero() {
                row.push(format!("{:.2}", f64::from(&qtys.requested)));
                row.push(empty.clone());
                row.push(format!("{:.2}", f64::from(&qtys.limit)));
                row.push(empty.clone());
                row.push(empty.clone());
                row.push(empty.clone());
            } else {
                row.push(format!("{:.2}", f64::from(&qtys.requested)));
                row.push(format!(
                    "{:.0}%",
                    qtys.requested.calc_percentage(&qtys.allocatable)
                ));
                row.push(format!("{:.2}", f64::from(&qtys.limit)));
                row.push(format!(
                    "{:.0}%",
                    qtys.limit.calc_percentage(&qtys.allocatable)
                ));
                row.push(format!("{:.2}", f64::from(&qtys.allocatable)));
                row.push(format!("{:.2}", f64::from(&qtys.calc_free())));
            }
        } else {
            row.push(empty.clone());
            row.push(empty.clone());
            row.push(empty.clone());
            row.push(empty.clone());
            row.push(empty.clone());
            row.push(empty.clone());
        }
        println!("{}", &row.join(","));
    }
}

fn display_with_prettytable(data: &[(Vec<String>, Option<QtyOfUsage>)], filter_full_zero: bool) {
    use prettytable::{cell, format, row, Cell, Row, Table};
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
    table.set_titles(row![bl->"Resource", br->"Requested", br->"%Requested", br->"Limit",  br->"%Limit", br->"Allocatable", br->"Free"]);
    let data2 = data
        .iter()
        .filter(|d| {
            !filter_full_zero
                || !d
                    .1
                    .as_ref()
                    .map(|x| x.requested.is_zero() && x.limit.is_zero() && x.allocatable.is_zero())
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
        let row = if let Some(qtys) = oqtys {
            if qtys.allocatable.is_zero() {
                let style = if qtys.requested.is_zero() || qtys.limit.is_zero() {
                    "rFr"
                } else {
                    "r"
                };
                Row::new(vec![
                    Cell::new(&column0),
                    Cell::new(&format!("{}", qtys.requested)).style_spec(style),
                    Cell::new("").style_spec(style),
                    Cell::new(&format!("{}", qtys.limit)).style_spec(style),
                    Cell::new("").style_spec(style),
                    Cell::new("").style_spec(style),
                    Cell::new("").style_spec(style),
                ])
            } else {
                row![
                    &column0,
                    r-> &format!("{}", qtys.requested),
                    r-> &format!("{:4.0}%", qtys.requested.calc_percentage(&qtys.allocatable)),
                    r-> &format!("{}", qtys.limit),
                    r-> &format!("{:4.0}%", qtys.limit.calc_percentage(&qtys.allocatable)),
                    r-> &format!("{}", qtys.allocatable.adjust_scale()),
                    r-> &format!("{}", qtys.calc_free().adjust_scale()),
                ]
            }
        } else {
            row![
                &column0,
                r-> "",
                r-> "",
                r-> "",
                r-> "",
                r-> "",
                r-> "",
            ]
        };
        table.add_row(row);
    }

    // Print the table to stdout
    table.printstd();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accept_resource() {
        assert_eq!(accept_resource("cpu", &vec![]), true);
        assert_eq!(accept_resource("cpu", &vec!["c".to_string()]), true);
        assert_eq!(accept_resource("cpu", &vec!["cpu".to_string()]), true);
        assert_eq!(accept_resource("cpu", &vec!["cpu3".to_string()]), false);
        assert_eq!(accept_resource("gpu", &vec!["gpu".to_string()]), true);
        assert_eq!(
            accept_resource("nvidia.com/gpu", &vec!["gpu".to_string()]),
            true
        );
    }
}
