use clap::Parser;
use kubectl_view_allocations::{do_main, CliOpts, GroupBy};
use tracing::error;
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;

fn init_tracing() {
    // std::env::set_var("RUST_LOG", "info,kube=trace");

    std::env::set_var(
        "RUST_LOG",
        std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".to_string()),
    );
    let formatting_layer =
        BunyanFormattingLayer::new(env!("CARGO_CRATE_NAME").to_owned(), std::io::stderr);
    let subscriber = Registry::default()
        .with(EnvFilter::from_default_env())
        .with(JsonStorageLayer)
        .with(formatting_layer);
    tracing::subscriber::set_global_default(subscriber).unwrap();
}

#[tokio::main]
async fn main() {
    init_tracing();
    let mut cli_opts = CliOpts::parse();
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
