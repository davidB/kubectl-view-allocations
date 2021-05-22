use env_logger;
use kubectl_view_allocations::{do_main, CliOpts, GroupBy};
use log::error;
use structopt::StructOpt;

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
