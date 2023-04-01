use clap::Parser;
use color_eyre::eyre::Result;
use kubectl_view_allocations::{do_main, CliOpts, GroupBy};

fn init_tracing() {
    // std::env::set_var("RUST_LOG", "info,kube=trace");
    use tracing_error::ErrorLayer;
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{fmt, EnvFilter};

    std::env::set_var(
        "RUST_LOG",
        std::env::var("RUST_LOG").unwrap_or_else(|_| "warn,kube_client=error".to_string()),
    );

    let fmt_layer = fmt::layer().with_target(false);
    let filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .with(ErrorLayer::default())
        .init();
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    color_eyre::config::HookBuilder::default()
        .panic_section("consider reporting the bug on github")
        .install()?;
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

    do_main(&cli_opts).await?;
    Ok(())
}
