use serde::{Deserialize, Serialize};
// kubectl get --raw /apis/metrics.k8s.io/v1beta1/pods | jq .

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub cpu: String,
    pub memory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Container {
    pub name: String,
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodMetrics {
    pub metadata: kube::api::ObjectMeta,
    pub containers: Vec<Container>,
    pub timestamp: String,
    pub window: String,
}

// custom impl since metrics API doesn't exist on kube-rs
impl k8s_openapi::Resource for PodMetrics {
    const GROUP: &'static str = "metrics.k8s.io";
    const KIND: &'static str = "pod";
    const VERSION: &'static str = "v1beta1";
    const API_VERSION: &'static str = "metrics.k8s.io/v1beta1";
    const URL_PATH_SEGMENT: &'static str = "pods";
    type Scope = k8s_openapi::NamespaceResourceScope;
}

impl k8s_openapi::Metadata for PodMetrics {
    type Ty = kube::api::ObjectMeta;

    fn metadata(&self) -> &Self::Ty {
        &self.metadata
    }

    fn metadata_mut(&mut self) -> &mut Self::Ty {
        &mut self.metadata
    }
}

// #[derive(Debug, Clone, Serialize, Deserialize)]
// struct PodMetricsList {
//     metadata: kube::api::ObjectMeta,
//     api_version: String,
//     kind: String,
//     items: Vec<PodMetrics>,
// }
