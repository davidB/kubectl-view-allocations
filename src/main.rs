mod qty;
mod tree;
// mod human_format;
use env_logger;
use failure::Error;
use qty::Qty;
use std::str::FromStr;
use itertools::Itertools;
use structopt::StructOpt;
use structopt::clap::AppSettings;

use kube::{
    api::{Api, ListParams},
    client::{APIClient},
    config,
};

#[derive(Debug,Clone,Default)]
struct Location {
    node_name: Option<String>,
    namespace: Option<String>,
    pod_name: Option<String>,
    container_name: Option<String>,
}

#[derive(Debug,Clone)]
struct Resource {
    kind: String,
    quantity: Qty,
    location: Location,
    usage: ResourceUsage,
}

#[derive(Debug,Clone)]
enum ResourceUsage {
    Limit,
    Requested,
    Allocatable,
}

#[derive(Debug,Clone,Default)]
struct QtyOfUsage {
    limit: Qty,
    requested: Qty,
    allocatable: Qty,
}

impl QtyOfUsage {
    pub fn calc_free(&self) -> Qty {
        let total_used = if self.limit > self.requested { &self.limit } else { &self.requested };
        if self.allocatable > *total_used {
            &self.allocatable - total_used
        } else {
            Qty::default()
        }
    }
}
fn sum_by_usage<'a>(rsrcs: &[&Resource]) -> QtyOfUsage {
    rsrcs.iter().fold(QtyOfUsage::default(), |mut acc, v|{
        match &v.usage {
            ResourceUsage::Limit => acc.limit += &v.quantity,
            ResourceUsage::Requested => acc.requested += &v.quantity,
            ResourceUsage::Allocatable => acc.allocatable += &v.quantity,
        };
        acc
    })
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

fn make_kind_x_usage(rsrcs: &[Resource]) -> Vec<(Vec<String>, QtyOfUsage)> {
    let group_by_fct: Vec<Box<dyn Fn(&Resource) -> Option<String>>> = vec![Box::new(extract_kind), Box::new(extract_node_name), Box::new(extract_pod_name)];
    let mut out = make_group_x_usage(&(rsrcs.iter().collect::<Vec<_>>()), &vec![], &group_by_fct, 0);
    out.sort_by_key(|i| i.0.clone());
    out
}

fn make_group_x_usage<F>(rsrcs: &[&Resource], prefix: &[String], group_by_fct: &[F], group_by_depth: usize) -> Vec<(Vec<String>, QtyOfUsage)>
where F: Fn(&Resource) -> Option<String>,
{
    // Note: The `&` is significant here, `GroupBy` is iterable
    // only by reference. You can also call `.into_iter()` explicitly.
    let mut out = vec![];
    if let Some(group_by) = group_by_fct.get(group_by_depth) {
        for (key, group) in rsrcs.iter().filter_map(|e| group_by(e).map(|k| (k, *e))).into_group_map() {
            let mut key_full = prefix.to_vec();
            key_full.push(key);
            // Check that the sum of each group is +/- 4.
            let children = make_group_x_usage(&group, &key_full, group_by_fct, group_by_depth + 1);
            out.push((key_full, sum_by_usage(&group)));
            out.extend(children);
        }
    }
    // let kg = &rsrcs.into_iter().group_by(|v| v.kind);
    // kg.into_iter().map(|(key, group)|  ).collect()
    out
}

fn accept_resource(name: &str, resource_filter: &Vec<String>) -> bool {
    resource_filter.is_empty() || resource_filter.iter().any(|x| name.contains(x))
}

fn collect_from_nodes(client: APIClient, resources: &mut Vec<Resource>, resource_names: &Vec<String>) -> Result<(), Error> {
    let api_nodes = Api::v1Node(client);//.within("default");
    let nodes = api_nodes.list(&ListParams::default())?;
    for node in nodes.items {
        let location = Location {
            node_name: Some(node.metadata.name.clone()),
            ..Location::default()
        };
        if let Some(als) = node.status.and_then(|v| v.allocatable) {
            for a in als.into_iter().filter(|a| accept_resource(&a.0, resource_names)) {
                resources.push(Resource{
                    kind: a.0,
                    usage: ResourceUsage::Allocatable,
                    quantity: Qty::from_str(&(a.1).0)?,
                    location: location.clone(),
                });
            }
        }
    }
    Ok(())
}

fn collect_from_pods(client: APIClient, resources: &mut Vec<Resource>, resource_names: &Vec<String>, namespace: &Option<String>) -> Result<(), Error> {
    let api_pods = if let Some(ns) = namespace {
        Api::v1Pod(client).within(ns)
    } else {
        Api::v1Pod(client)
    };
    let pods = api_pods.list(&ListParams::default())?;
    for pod in pods.items {
        let node_name = pod.status.and_then(|v| v.nominated_node_name).or(pod.spec.node_name);
        for container in pod.spec.containers {
            let location = Location{
                node_name: node_name.clone(),
                namespace: pod.metadata.namespace.clone(),
                pod_name: Some(pod.metadata.name.clone()),
                container_name: Some(container.name.clone()),
            };
            for requirements in container.resources {
                if let Some(r) = requirements.requests {
                    for request in r.into_iter().filter(|a| accept_resource(&a.0, resource_names)) {
                        resources.push(Resource{
                            kind: request.0,
                            usage: ResourceUsage::Requested,
                            quantity: Qty::from_str(&(request.1).0)?,
                            location: location.clone(),
                        });
                    }
                }
                if let Some(l) = requirements.limits {
                    for limit in l.into_iter().filter(|a| accept_resource(&a.0, resource_names)) {
                        resources.push(Resource{
                            kind: limit.0,
                            usage: ResourceUsage::Limit,
                            quantity: Qty::from_str(&(limit.1).0)?,
                            location: location.clone(),
                        });
                    }
                }
            }
        }
    }
    Ok(())
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
}

fn main() -> Result<(),Error> {
    let cli_opts = CliOpts::from_args();
    // dbg!(&cli_opts);

    // std::env::set_var("RUST_LOG", "info,kube=trace");
    env_logger::init();
    let config = config::load_kube_config().expect("failed to load kubeconfig");
    let client = APIClient::new(config);

    let mut resources: Vec<Resource> = vec![];
    collect_from_nodes(client.clone(), &mut resources, &cli_opts.resource_name)?;
    collect_from_pods(client.clone(), &mut resources, &cli_opts.resource_name, &cli_opts.namespace)?;

    let res = make_kind_x_usage(&resources);
    display_with_prettytable(&res, !&cli_opts.show_zero);
    Ok(())
}

fn display_with_prettytable(data: &[(Vec<String>, QtyOfUsage)], filter_full_zero: bool) {
    use prettytable::{Table, row, cell, format, Cell, Row};
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
    let data2 = data.iter().filter(|d| !filter_full_zero || !d.1.requested.is_zero() || !d.1.limit.is_zero() || !d.1.allocatable.is_zero()).collect::<Vec<_>>();
    let prefixes = tree::provide_prefix(&data2, |parent, item|{
        parent.0.len() + 1 == item.0.len()
    });

    for ((k, qtys), prefix) in data2.iter().zip(prefixes.iter()) {
        let row = if qtys.allocatable.is_zero() {
            let style = if qtys.requested.is_zero() || qtys.limit.is_zero() {
                "rFr"
            } else {
                "r"
            };
            Row::new(vec![
                Cell::new(&format!("{} {}", prefix, k.last().map(|x| x.as_str()).unwrap_or("???"))),
                Cell::new(&format!("{}", qtys.requested.adjust_scale())).style_spec(style),
                Cell::new("").style_spec(style),
                Cell::new(&format!("{}", qtys.limit.adjust_scale())).style_spec(style),
                Cell::new("").style_spec(style),
                Cell::new("").style_spec(style),
                Cell::new("").style_spec(style),
            ])
        } else {
            row![
                &format!("{} {}", prefix, k.last().map(|x| x.as_str()).unwrap_or("???")),
                r-> &format!("{}", qtys.requested.adjust_scale()),
                r-> &format!("{:4.0}%", qtys.requested.calc_percentage(&qtys.allocatable)),
                r-> &format!("{}", qtys.limit.adjust_scale()),
                r-> &format!("{:4.0}%", qtys.limit.calc_percentage(&qtys.allocatable)),
                r-> &format!("{}", qtys.allocatable.adjust_scale()),
                r-> &format!("{}", qtys.calc_free().adjust_scale()),
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
        assert_eq!(accept_resource("nvidia.com/gpu", &vec!["gpu".to_string()]), true);
    }
}
