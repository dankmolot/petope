use crate::{config::Config, peer::ALPN, router::Router};
use anyhow::{Context, Result};
use clap::Parser;
use iroh::{Endpoint, endpoint::presets};

mod config;
mod peer;
mod peer_addr;
mod router;
mod tun;
mod utils;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Path to a config file
    #[arg(short, long, default_value_t = String::from("config.toml"))]
    config: String,
}

#[tokio::main()]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let (secret_key, config) = Config::load(&cli.config).context("load config")?;

    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(secret_key)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .context("bind an endpoint")?;

    let router = Router::new(&config, endpoint).context("Router::new")?;

    router.run().await
}
