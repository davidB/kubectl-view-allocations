//! Dynamic Resource Allocation (DRA, `resource.k8s.io`) support.
//!
//! - capacity (allocatable) = devices published in `ResourceSlice`s (latest generation of each pool)
//! - usage (requested & limit) = devices allocated to `ResourceClaim`s, attributed to the node owning
//!   the device and to the pods the claim is reserved for
//!
//! Only the fields needed are deserialized, they have the same shape in `v1`, `v1beta2` and `v1beta1`,
//! so the version served by the cluster (found by discovery) is used without depending on the
//! k8s-openapi feature (version) selected at build time.
//! kubectl get resourceslices,resourceclaims,deviceclasses -A -o json | jq .
use crate::qty::Qty;
use crate::{Error, Location, POD_NAMES_SEPARATOR, Resource, ResourceQualifier};
use futures::future::try_join_all;
use itertools::Itertools;
use kube::api::{Api, DynamicObject, ListParams, ObjectMeta};
use kube::discovery::ApiGroup;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::str::FromStr;
use tracing::{info, instrument, warn};

const API_GROUP: &str = "resource.k8s.io";
/// annotation set on the ResourceClaim created by the scheduler for a pod requesting an extended
/// resource backed by DRA (KEP-5004)
const EXTENDED_RESOURCE_CLAIM_ANNOTATION: &str = "resource.kubernetes.io/extended-resource-claim";
/// extended resource name usable for every DeviceClass (KEP-5004)
const IMPLICIT_EXTENDED_RESOURCE_PREFIX: &str = "deviceclass.resource.kubernetes.io/";
/// prefix of the pseudo node name used for devices not attached to a single node
const POOL_NODE_PREFIX: &str = "pool:";

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ResourceSlice {
    pub spec: ResourceSliceSpec,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceSliceSpec {
    pub driver: String,
    pub pool: ResourcePool,
    pub node_name: Option<String>,
    #[serde(default)]
    pub devices: Vec<Device>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ResourcePool {
    pub name: String,
    #[serde(default)]
    pub generation: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Device {
    pub name: String,
    /// set when the slice uses `perDeviceNodeSelection`
    pub node_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ResourceClaim {
    #[serde(default)]
    pub metadata: ObjectMeta,
    #[serde(default)]
    pub spec: ResourceClaimSpec,
    pub status: Option<ResourceClaimStatus>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ResourceClaimSpec {
    pub devices: Option<DeviceClaim>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct DeviceClaim {
    #[serde(default)]
    pub requests: Vec<DeviceRequest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceRequest {
    /// v1beta1
    pub device_class_name: Option<String>,
    /// v1beta2, v1
    pub exactly: Option<ExactDeviceRequest>,
    #[serde(default)]
    pub first_available: Vec<DeviceSubRequest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExactDeviceRequest {
    pub device_class_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceSubRequest {
    pub device_class_name: Option<String>,
}

impl DeviceRequest {
    fn device_class_names(&self) -> Vec<&str> {
        self.device_class_name
            .as_deref()
            .or_else(|| {
                self.exactly
                    .as_ref()
                    .and_then(|e| e.device_class_name.as_deref())
            })
            .into_iter()
            .chain(
                self.first_available
                    .iter()
                    .filter_map(|s| s.device_class_name.as_deref()),
            )
            .collect()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceClaimStatus {
    pub allocation: Option<AllocationResult>,
    #[serde(default)]
    pub reserved_for: Vec<ResourceClaimConsumerReference>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AllocationResult {
    pub devices: Option<DeviceAllocationResult>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct DeviceAllocationResult {
    #[serde(default)]
    pub results: Vec<DeviceRequestAllocationResult>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceRequestAllocationResult {
    pub driver: String,
    pub pool: String,
    pub device: String,
    /// admin access (eg. monitoring) doesn't consume the device
    pub admin_access: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceClaimConsumerReference {
    #[serde(default)]
    pub api_group: Option<String>,
    pub resource: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct DeviceClass {
    #[serde(default)]
    pub metadata: ObjectMeta,
    #[serde(default)]
    pub spec: DeviceClassSpec,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceClassSpec {
    pub extended_resource_name: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DraObjects {
    pub slices: Vec<ResourceSlice>,
    pub claims: Vec<ResourceClaim>,
    pub classes: Vec<DeviceClass>,
}

/// Collect DRA devices: allocatable from `ResourceSlice`, requested & limit from `ResourceClaim`.
///
/// Do nothing when the cluster doesn't serve the `resource.k8s.io` API.
/// Must be called after [`crate::collect_from_pods`]: requests of extended resources backed by DRA
/// are removed from the pods, as they are counted from their ResourceClaim.
#[instrument(skip(client, resources))]
pub async fn collect_from_dra(
    client: kube::Client,
    resources: &mut Vec<Resource>,
    namespace: &[String],
    selected_node_names: &[String],
) -> Result<(), Error> {
    let group = match kube::discovery::group(&client, API_GROUP).await {
        Ok(group) => group,
        Err(kube::Error::Discovery(err)) => {
            info!(?err, "DRA API not available");
            return Ok(());
        }
        Err(source) => {
            return Err(Error::KubeError {
                context: format!("discover {API_GROUP} API"),
                source,
            });
        }
    };
    let (slices, claims) = futures::try_join!(
        list_objects::<ResourceSlice>(&client, &group, "ResourceSlice", &[]),
        list_objects::<ResourceClaim>(&client, &group, "ResourceClaim", namespace),
    )?;
    // only used to resolve extended resource names, not fatal
    let classes = list_objects::<DeviceClass>(&client, &group, "DeviceClass", &[])
        .await
        .unwrap_or_else(|err| {
            warn!(?err);
            vec![]
        });
    extract_from_dra(
        DraObjects {
            slices,
            claims,
            classes,
        },
        resources,
        selected_node_names,
    )
}

async fn list_objects<T: DeserializeOwned>(
    client: &kube::Client,
    group: &ApiGroup,
    kind: &str,
    namespace: &[String],
) -> Result<Vec<T>, Error> {
    let Some((ar, _caps)) = group.recommended_kind(kind) else {
        info!(kind, "kind not available in {API_GROUP}");
        return Ok(vec![]);
    };
    let apis: Vec<Api<DynamicObject>> = if namespace.is_empty() {
        vec![Api::all_with(client.clone(), &ar)]
    } else {
        namespace
            .iter()
            .map(|ns| Api::namespaced_with(client.clone(), ns, &ar))
            .collect()
    };
    let lists = try_join_all(
        apis.iter()
            .map(|api| async { api.list(&ListParams::default()).await }),
    )
    .await
    .map_err(|source| Error::KubeError {
        context: format!(
            "list {}.{} (DRA), use `--no-dra` to skip",
            ar.plural, ar.version
        ),
        source,
    })?;
    Ok(lists
        .into_iter()
        .flat_map(|list| list.items)
        .filter_map(|obj| {
            // an unexpected object should not prevent displaying the others
            serde_json::to_value(&obj)
                .and_then(serde_json::from_value::<T>)
                .inspect_err(|err| warn!(?err, kind, name = ?obj.metadata.name, "ignored"))
                .ok()
        })
        .collect())
}

type DeviceKey = (String, String, String);

#[allow(clippy::result_large_err)]
#[instrument(skip(objects, resources))]
pub(crate) fn extract_from_dra(
    objects: DraObjects,
    resources: &mut Vec<Resource>,
    selected_node_names: &[String],
) -> Result<(), Error> {
    let one = Qty::from_str("1")?;
    let is_selected = |node_name: &str| {
        node_name.starts_with(POOL_NODE_PREFIX)
            || selected_node_names.iter().any(|n| n == node_name)
    };

    // a pool can be updated by publishing slices with a new generation, older ones are obsolete
    let mut pool_generations: HashMap<(&str, &str), i64> = HashMap::new();
    for slice in &objects.slices {
        let key = (slice.spec.driver.as_str(), slice.spec.pool.name.as_str());
        let generation = pool_generations
            .entry(key)
            .or_insert(slice.spec.pool.generation);
        *generation = (*generation).max(slice.spec.pool.generation);
    }
    let mut device_nodes: HashMap<DeviceKey, String> = HashMap::new();
    for slice in &objects.slices {
        let spec = &slice.spec;
        if pool_generations.get(&(spec.driver.as_str(), spec.pool.name.as_str()))
            != Some(&spec.pool.generation)
        {
            continue;
        }
        for device in &spec.devices {
            let node_name = device
                .node_name
                .clone()
                .or_else(|| spec.node_name.clone())
                .unwrap_or_else(|| pool_node_name(&spec.pool.name));
            if is_selected(&node_name) {
                resources.push(Resource {
                    kind: spec.driver.clone(),
                    quantity: one.clone(),
                    location: Location {
                        node_name: node_name.clone(),
                        ..Location::default()
                    },
                    qualifier: ResourceQualifier::Allocatable,
                });
            }
            device_nodes.insert(
                (
                    spec.driver.clone(),
                    spec.pool.name.clone(),
                    device.name.clone(),
                ),
                node_name,
            );
        }
    }

    let class_resource_names: HashMap<&str, String> = objects
        .classes
        .iter()
        .filter_map(|c| {
            let name = c.metadata.name.as_deref()?;
            c.spec
                .extended_resource_name
                .clone()
                .map(|resource_name| (name, resource_name))
        })
        .collect();
    // (namespace, pod_name) -> extended resources already counted by a ResourceClaim
    let mut extended_resources_by_pod: HashMap<(String, String), BTreeSet<String>> = HashMap::new();
    // a device can be shared by several claims (consumable capacity), count it once
    let mut counted_devices: HashSet<DeviceKey> = HashSet::new();
    for claim in &objects.claims {
        let Some(status) = &claim.status else {
            continue;
        };
        let Some(results) = status
            .allocation
            .as_ref()
            .and_then(|a| a.devices.as_ref())
            .map(|d| &d.results)
        else {
            continue;
        };
        let claim_name = claim.metadata.name.clone().unwrap_or_default();
        let namespace = claim.metadata.namespace.clone();
        let pod_names = status
            .reserved_for
            .iter()
            .filter(|c| c.resource == "pods" && c.api_group.as_deref().unwrap_or("").is_empty())
            .map(|c| c.name.clone())
            .collect::<Vec<_>>();
        let consumers = status
            .reserved_for
            .iter()
            .map(|c| {
                if c.resource == "pods" {
                    c.name.clone()
                } else {
                    format!("{}/{}", c.resource, c.name)
                }
            })
            .join(POD_NAMES_SEPARATOR);
        let pod_name = if consumers.is_empty() {
            format!("claim/{claim_name}")
        } else {
            consumers
        };

        for result in results.iter().filter(|r| r.admin_access != Some(true)) {
            let key = (
                result.driver.clone(),
                result.pool.clone(),
                result.device.clone(),
            );
            let node_name = device_nodes.get(&key).cloned().unwrap_or_else(|| {
                // the pool of node-local devices is usually named after the node
                if selected_node_names.contains(&result.pool) {
                    result.pool.clone()
                } else {
                    pool_node_name(&result.pool)
                }
            });
            if !is_selected(&node_name) {
                continue;
            }
            let quantity = if counted_devices.insert(key) {
                one.clone()
            } else {
                Qty::zero()
            };
            let location = Location {
                node_name,
                namespace: namespace.clone(),
                pod_name: Some(pod_name.clone()),
                claim: Some(claim_name.clone()),
            };
            // devices are allocated exclusively, so limit == requested
            for qualifier in [ResourceQualifier::Requested, ResourceQualifier::Limit] {
                resources.push(Resource {
                    kind: result.driver.clone(),
                    quantity: quantity.clone(),
                    location: location.clone(),
                    qualifier,
                });
            }
        }

        if is_extended_resource_claim(&claim.metadata) {
            let resource_names = claim
                .spec
                .devices
                .iter()
                .flat_map(|d| &d.requests)
                .flat_map(DeviceRequest::device_class_names)
                .flat_map(|class_name| {
                    [
                        class_resource_names.get(class_name).cloned(),
                        Some(format!("{IMPLICIT_EXTENDED_RESOURCE_PREFIX}{class_name}")),
                    ]
                })
                .flatten()
                .collect::<Vec<_>>();
            for pod_name in &pod_names {
                extended_resources_by_pod
                    .entry((namespace.clone().unwrap_or_default(), pod_name.clone()))
                    .or_default()
                    .extend(resource_names.iter().cloned());
            }
        }
    }

    remove_extended_resources_from_pods(resources, &extended_resources_by_pod);
    Ok(())
}

fn pool_node_name(pool_name: &str) -> String {
    format!("{POOL_NODE_PREFIX}{pool_name}")
}

fn is_extended_resource_claim(metadata: &ObjectMeta) -> bool {
    metadata
        .annotations
        .as_ref()
        .and_then(|a| a.get(EXTENDED_RESOURCE_CLAIM_ANNOTATION))
        .is_some_and(|v| v == "true")
}

/// The scheduler satisfies requests of extended resources backed by DRA with a ResourceClaim,
/// remove the requests & limits declared by containers to not count those devices twice.
fn remove_extended_resources_from_pods(
    resources: &mut Vec<Resource>,
    extended_resources_by_pod: &HashMap<(String, String), BTreeSet<String>>,
) {
    if extended_resources_by_pod.is_empty() {
        return;
    }
    resources.retain(|r| {
        let from_pod_spec = r.location.claim.is_none()
            && matches!(
                r.qualifier,
                ResourceQualifier::Requested | ResourceQualifier::Limit
            );
        let counted_by_claim = || {
            r.location.pod_name.as_ref().is_some_and(|pod_name| {
                extended_resources_by_pod
                    .get(&(
                        r.location.namespace.clone().unwrap_or_default(),
                        pod_name.clone(),
                    ))
                    .is_some_and(|names| names.contains(&r.kind))
            })
        };
        !(from_pod_spec && counted_by_claim())
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn slice(
        node_name: Option<&str>,
        pool: &str,
        generation: i64,
        devices: &[&str],
    ) -> ResourceSlice {
        serde_json::from_value(json!({
            "metadata": { "name": format!("{pool}-{generation}") },
            "spec": {
                "driver": "gpu.nvidia.com",
                "pool": { "name": pool, "generation": generation, "resourceSliceCount": 1 },
                "nodeName": node_name,
                "devices": devices.iter().map(|d| json!({ "name": d })).collect::<Vec<_>>(),
            }
        }))
        .unwrap()
    }

    fn claim(name: &str, devices: &[(&str, &str)], pods: &[&str]) -> ResourceClaim {
        serde_json::from_value(json!({
            "metadata": { "name": name, "namespace": "ns1" },
            "spec": { "devices": { "requests": [
                { "name": "gpu", "exactly": { "deviceClassName": "gpu.nvidia.com" } }
            ] } },
            "status": {
                "allocation": { "devices": { "results": devices.iter().map(|(pool, device)| json!({
                    "request": "gpu", "driver": "gpu.nvidia.com", "pool": pool, "device": device
                })).collect::<Vec<_>>() } },
                "reservedFor": pods.iter().map(|p| json!({ "resource": "pods", "name": p, "uid": p })).collect::<Vec<_>>(),
            }
        }))
        .unwrap()
    }

    /// (qualifier, node, pod) -> quantity
    fn summarize(resources: &[Resource]) -> BTreeMap<(String, String, String), String> {
        let mut sums: BTreeMap<(String, String, String), Qty> = BTreeMap::new();
        for r in resources {
            let key = (
                format!("{}/{:?}", r.kind, r.qualifier),
                r.location.node_name.clone(),
                r.location.pod_name.clone().unwrap_or_default(),
            );
            *sums.entry(key).or_insert_with(Qty::zero) += &r.quantity;
        }
        sums.into_iter()
            .map(|(k, v)| (k, format!("{}", v.adjust_scale())))
            .collect()
    }

    fn key(qualifier: &str, node: &str, pod: &str) -> (String, String, String) {
        (
            format!("gpu.nvidia.com/{qualifier}"),
            node.to_string(),
            pod.to_string(),
        )
    }

    fn selected_nodes() -> Vec<String> {
        vec!["node-a".to_string(), "node-b".to_string()]
    }

    #[test]
    fn allocatable_from_latest_slices_of_selected_nodes() {
        let objects = DraObjects {
            slices: vec![
                slice(Some("node-a"), "node-a", 0, &["gpu-0", "gpu-1", "gpu-2"]),
                slice(Some("node-a"), "node-a", 1, &["gpu-0", "gpu-1"]),
                slice(Some("node-b"), "node-b", 1, &["gpu-0", "gpu-1"]),
                slice(Some("node-c"), "node-c", 1, &["gpu-0"]),
                slice(None, "fabric", 1, &["gpu-0"]),
            ],
            ..DraObjects::default()
        };
        let mut resources = vec![];
        extract_from_dra(objects, &mut resources, &selected_nodes()).unwrap();
        assert_eq!(
            summarize(&resources),
            BTreeMap::from([
                (key("Allocatable", "node-a", ""), "2.0".to_string()),
                (key("Allocatable", "node-b", ""), "2.0".to_string()),
                (key("Allocatable", "pool:fabric", ""), "1.0".to_string()),
            ])
        );
    }

    #[test]
    fn requested_from_claims_counts_each_device_once() {
        let mut admin = claim("monitoring", &[("node-b", "gpu-1")], &["dcgm-exporter"]);
        admin
            .status
            .as_mut()
            .unwrap()
            .allocation
            .as_mut()
            .unwrap()
            .devices
            .as_mut()
            .unwrap()
            .results[0]
            .admin_access = Some(true);
        let mut pending = claim("pending", &[], &[]);
        pending.status = None;
        let objects = DraObjects {
            slices: vec![
                slice(Some("node-a"), "node-a", 1, &["gpu-0", "gpu-1"]),
                slice(Some("node-b"), "node-b", 1, &["gpu-0", "gpu-1"]),
                slice(Some("node-c"), "node-c", 1, &["gpu-0"]),
            ],
            claims: vec![
                claim(
                    "shared",
                    &[("node-a", "gpu-0"), ("node-a", "gpu-1")],
                    &["router", "worker-0"],
                ),
                claim("single", &[("node-b", "gpu-0")], &["ssh"]),
                claim("same-device", &[("node-b", "gpu-0")], &["ssh-bis"]),
                claim("unreserved", &[("node-b", "gpu-1")], &[]),
                claim("unselected-node", &[("node-c", "gpu-0")], &["other"]),
                admin,
                pending,
            ],
            ..DraObjects::default()
        };
        let mut resources = vec![];
        extract_from_dra(objects, &mut resources, &selected_nodes()).unwrap();
        let summary = summarize(&resources);
        let requested = summary
            .into_iter()
            .filter(|(k, _)| k.0.ends_with("Requested"))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            requested,
            BTreeMap::from([
                (
                    key("Requested", "node-a", "router, worker-0"),
                    "2.0".to_string()
                ),
                (key("Requested", "node-b", "ssh"), "1.0".to_string()),
                (key("Requested", "node-b", "ssh-bis"), "0.0".to_string()),
                (
                    key("Requested", "node-b", "claim/unreserved"),
                    "1.0".to_string()
                ),
            ])
        );
        let shared = resources
            .iter()
            .find(|r| r.location.pod_name.as_deref() == Some("router, worker-0"))
            .unwrap();
        assert_eq!(shared.location.claim.as_deref(), Some("shared"));
        assert_eq!(shared.location.namespace.as_deref(), Some("ns1"));
        assert!(
            resources
                .iter()
                .any(|r| matches!(r.qualifier, ResourceQualifier::Limit))
        );
    }

    #[test]
    fn claim_without_slice_falls_back_to_pool_name() {
        let objects = DraObjects {
            claims: vec![
                claim("on-node", &[("node-a", "gpu-0")], &["p1"]),
                claim("on-pool", &[("fabric", "gpu-0")], &["p2"]),
            ],
            ..DraObjects::default()
        };
        let mut resources = vec![];
        extract_from_dra(objects, &mut resources, &selected_nodes()).unwrap();
        let nodes = resources
            .iter()
            .map(|r| r.location.node_name.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(nodes, BTreeSet::from(["node-a", "pool:fabric"]));
    }

    #[test]
    fn extended_resource_requests_are_counted_from_their_claim() {
        let pod_resource = |kind: &str, qualifier: ResourceQualifier| Resource {
            kind: kind.to_string(),
            quantity: Qty::from_str("1").unwrap(),
            location: Location {
                node_name: "node-a".to_string(),
                namespace: Some("ns1".to_string()),
                pod_name: Some("ssh".to_string()),
                ..Location::default()
            },
            qualifier,
        };
        let mut resources = vec![
            pod_resource("nvidia.com/gpu", ResourceQualifier::Requested),
            pod_resource("nvidia.com/gpu", ResourceQualifier::Limit),
            pod_resource("cpu", ResourceQualifier::Requested),
            pod_resource("pods", ResourceQualifier::Requested),
        ];
        let mut extended = claim(
            "ssh-extended-resources-x7k2p",
            &[("node-a", "gpu-0")],
            &["ssh"],
        );
        extended.metadata.annotations = Some(BTreeMap::from([(
            EXTENDED_RESOURCE_CLAIM_ANNOTATION.to_string(),
            "true".to_string(),
        )]));
        let classes = vec![
            serde_json::from_value(json!({
                "metadata": { "name": "gpu.nvidia.com" },
                "spec": { "extendedResourceName": "nvidia.com/gpu" }
            }))
            .unwrap(),
        ];
        let objects = DraObjects {
            slices: vec![slice(Some("node-a"), "node-a", 1, &["gpu-0"])],
            claims: vec![extended],
            classes,
        };
        extract_from_dra(objects, &mut resources, &selected_nodes()).unwrap();
        let kinds = resources
            .iter()
            .map(|r| (r.kind.as_str(), format!("{:?}", r.qualifier)))
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                ("cpu", "Requested".to_string()),
                ("pods", "Requested".to_string()),
                ("gpu.nvidia.com", "Allocatable".to_string()),
                ("gpu.nvidia.com", "Requested".to_string()),
                ("gpu.nvidia.com", "Limit".to_string()),
            ]
        );
    }

    #[test]
    fn device_class_names_of_all_api_versions() {
        let requests: Vec<DeviceRequest> = serde_json::from_value(json!([
            { "name": "v1beta1", "deviceClassName": "a" },
            { "name": "v1", "exactly": { "deviceClassName": "b" } },
            { "name": "alt", "firstAvailable": [
                { "name": "x", "deviceClassName": "c" }, { "name": "y", "deviceClassName": "d" }
            ] },
        ]))
        .unwrap();
        let names = requests
            .iter()
            .flat_map(DeviceRequest::device_class_names)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["a", "b", "c", "d"]);
    }
}
