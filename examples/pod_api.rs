use k8s_openapi::api::core::v1::Pod;

use kube::{
    api::{Api, ListParams},
    Client,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    //std::env::set_var("RUST_LOG", "info,kube=debug");
    tracing_subscriber::fmt::init();
    let client = Client::try_default().await?;
    let pods: Api<Pod> = Api::all(client);
    // let pods: Api<Pod> = Api::namespaced(client, "kube-system");

    let lp = ListParams::default().timeout(10);
    let pods = pods.list(&lp).await?;

    eprintln!("pods: {:?}", pods);

    Ok(())
}
