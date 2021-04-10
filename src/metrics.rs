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

// #[derive(Debug, Clone, Serialize, Deserialize)]
// struct PodMetricsList {
//     metadata: kube::api::ObjectMeta,
//     api_version: String,
//     kind: String,
//     items: Vec<PodMetrics>,
// }
