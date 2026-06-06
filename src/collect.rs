use crate::qty::Qty;
use crate::{Error, Location, Resource, ResourceQualifier};
use futures::future::try_join_all;
use k8s_openapi::api::core::v1::{Node, Pod};
use kube::api::{Api, ListParams, ObjectList};
use std::collections::BTreeMap;
use std::str::FromStr;
use tracing::instrument;

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

    let filtered_nodes: Vec<Node> = all_nodes
        .into_iter()
        .filter(|node| should_include_node_by_taint(node, ignore_taints))
        .collect();

    let node_names = filtered_nodes
        .iter()
        .filter_map(|node| node.metadata.name.clone())
        .collect();
    extract_allocatable_from_nodes(filtered_nodes, resources)?;
    Ok(node_names)
}

#[allow(clippy::result_large_err)]
#[instrument(skip(node_list, resources))]
pub fn extract_allocatable_from_nodes(
    node_list: Vec<Node>,
    resources: &mut Vec<Resource>,
) -> Result<(), Error> {
    for node in node_list {
        let location = Location {
            node_name: node.metadata.name.unwrap_or_default(),
            ..Location::default()
        };
        if let Some(als) = node.status.and_then(|v| v.allocatable) {
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

pub fn is_scheduled(pod: &Pod) -> bool {
    pod.status
        .as_ref()
        .and_then(|ps| {
            ps.phase.as_ref().and_then(|phase| {
                match &phase[..] {
                    "Succeeded" | "Failed" => Some(false),
                    "Running" => Some(true),
                    "Unknown" => None, // kubelet not responding
                    "Pending" => ps.conditions.as_ref().map(|o| {
                        o.iter()
                            .any(|c| c.type_ == "PodScheduled" && c.status == "True")
                    }),
                    &_ => None,
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
            *current_quantity = op(current_quantity.clone(), quantity);
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

    extract_allocatable_from_pods(pods, resources, selected_node_names)?;
    Ok(())
}

#[allow(clippy::result_large_err)]
#[instrument(skip(pod_list, resources))]
pub fn extract_allocatable_from_pods(
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
        // see https://kubernetes.io/docs/concepts/workloads/pods/init-containers/#resources
        let mut resource_requests: BTreeMap<String, Qty> = BTreeMap::new();
        let mut resource_limits: BTreeMap<String, Qty> = BTreeMap::new();
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
        if let Some(ref overhead) = spec.and_then(|s| s.overhead.clone()) {
            process_resources(&mut resource_requests, overhead, std::ops::Add::add)?;
            process_resources(&mut resource_limits, overhead, std::ops::Add::add)?;
        }
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

#[instrument(skip(client, resources))]
pub async fn collect_from_metrics(
    client: kube::Client,
    resources: &mut Vec<Resource>,
) -> Result<(), Error> {
    let api_pod_metrics: Api<crate::metrics::PodMetrics> = Api::all(client);
    let pod_metrics = api_pod_metrics
        .list(&ListParams::default())
        .await
        .map_err(|source| Error::KubeError {
            context: "list podmetrics, maybe Metrics API not available".to_string(),
            source,
        })?;

    extract_utilizations_from_pod_metrics(pod_metrics, resources)?;
    Ok(())
}

#[allow(clippy::result_large_err)]
#[instrument(skip(pod_metrics, resources))]
pub fn extract_utilizations_from_pod_metrics(
    pod_metrics: ObjectList<crate::metrics::PodMetrics>,
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

pub fn should_include_node_by_taint(node: &Node, ignore_taints: &Option<Vec<String>>) -> bool {
    let taints = node
        .spec
        .as_ref()
        .and_then(|spec| spec.taints.as_ref())
        .map(|taints| taints.as_slice())
        .unwrap_or(&[]);

    match ignore_taints {
        None => taints.is_empty(),
        Some(patterns) if patterns.is_empty() => true,
        Some(patterns) => {
            if taints.is_empty() {
                return true;
            }
            for taint in taints {
                let taint_key = taint.key.as_str();
                let taint_value = taint.value.as_deref();

                for ignore_pattern in patterns {
                    if ignore_pattern == taint_key {
                        return true;
                    }
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
            false
        }
    }
}
