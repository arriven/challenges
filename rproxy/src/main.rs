use clap::Parser;
use std::{path::PathBuf, str::FromStr};
use tracing_subscriber::{filter::targets::Targets, layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod proxy;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Name of the person to greet
    #[arg(short, long, env = "RPROXY_CONFIG_FILE_PATH")]
    config_file_path: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter_layer =
        Targets::from_str(std::env::var("RUST_LOG").as_deref().unwrap_or("info")).unwrap();
    let format_layer = tracing_subscriber::fmt::layer();
    tracing_subscriber::registry()
        .with(filter_layer)
        .with(format_layer)
        .init();

    let args = Args::parse();

    let config = config::Cfg::try_build(&args.config_file_path)?;
    let handles: Vec<_> = config
        .apps
        .into_iter()
        .map(|app| tokio::spawn(proxy::run(app)))
        .collect();
    futures::future::join_all(handles).await;
    Ok(())
}
