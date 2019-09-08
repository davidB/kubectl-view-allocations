mod qty;
mod human_format;
use env_logger;
use failure::Error;
use qty::Qty;
use std::str::FromStr;
use itertools::Itertools;

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

fn make_kind_x_usage(rsrcs: &[Resource]) -> Vec<(String, QtyOfUsage)> {
    // Note: The `&` is significant here, `GroupBy` is iterable
    // only by reference. You can also call `.into_iter()` explicitly.
    let mut out = vec![];
    for (key, group) in rsrcs.into_iter().map(|e| (e.kind.clone(), e)).into_group_map() {
        // Check that the sum of each group is +/- 4.
        out.push((key, sum_by_usage(&group)));
    }
    // let kg = &rsrcs.into_iter().group_by(|v| v.kind);
    // kg.into_iter().map(|(key, group)|  ).collect()
    out.sort_by_key(|i| i.0.clone());
    out
}

fn collect_from_nodes(client: APIClient, resources: &mut Vec<Resource>) -> Result<(), Error> {
    let api_nodes = Api::v1Node(client);//.within("default");
    let nodes = api_nodes.list(&ListParams::default())?;
    for node in nodes.items {
        let location = Location {
            node_name: Some(node.metadata.name.clone()),
            ..Location::default()
        };
        if let Some(als) = node.status.and_then(|v| v.allocatable) {
            for a in als {
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

fn collect_from_pods(client: APIClient, resources: &mut Vec<Resource>) -> Result<(), Error> {
    let api_pods = Api::v1Pod(client);//.within("default");
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
                    for request in r {
                        resources.push(Resource{
                            kind: request.0,
                            usage: ResourceUsage::Requested,
                            quantity: Qty::from_str(&(request.1).0)?,
                            location: location.clone(),
                        });
                    }
                }
                if let Some(l) = requirements.limits {
                    for limit in l {
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
fn main() -> Result<(),Error> {
    // std::env::set_var("RUST_LOG", "info,kube=trace");
    env_logger::init();
    let config = config::load_kube_config().expect("failed to load kubeconfig");
    let client = APIClient::new(config);

    let mut resources: Vec<Resource> = vec![];
    collect_from_nodes(client.clone(), &mut resources)?;
    collect_from_pods(client.clone(), &mut resources)?;

    let res = make_kind_x_usage(&resources);
    // display_with_tabwriter(&res);
    display_with_prettytable(&res);
    Ok(())
}

// fn display_with_tabwriter(data: &[(String, QtyOfUsage)]) {
//     use tabwriter::TabWriter;
//     use std::io::Write;
//     let mut tw = TabWriter::new(vec![]);
//     tw.write(b"\tRequested\tLimit\tAllocatable\n")?;
//     for (k, qtys) in data {
//         tw.write_fmt(format_args!("{}\t{}\t{}\t{}\n", k, qtys.requested, qtys.limit, qtys.allocatable))?;
//     }
//     tw.flush()?;
//     println!("{}", String::from_utf8(tw.into_inner()?)?);
// }

fn display_with_prettytable(data: &[(String, QtyOfUsage)]) {
    use prettytable::{Table, row, cell, format};
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

    for (k, qtys) in data {
        table.add_row(row![
            k,
            r-> &format!("{}", qtys.requested),
            r-> &format!("{:3.0}", qtys.requested.calc_percentage(&qtys.allocatable)),
            r-> &format!("{}", qtys.limit),
            r-> &format!("{:3.0}", qtys.limit.calc_percentage(&qtys.allocatable)),
            r-> &format!("{}", qtys.allocatable),
            r-> &format!("{}", qtys.calc_free()),
        ]);
    }

    // Print the table to stdout
    table.printstd();
}
