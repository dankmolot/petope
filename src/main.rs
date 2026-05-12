use std::borrow::Cow;
use crate::{config::Config, peer::ALPN, peer_addr::PeerAddr, router::Router};
use anyhow::{Context, Result};
use clap::Parser;
use iroh::{Endpoint, PublicKey, TransportAddr, endpoint::presets, endpoint_info::AddrFilter};
use log::info;

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
    configure_logging();

    let (secret_key, config) = Config::load(&cli.config).context("load config")?;
    let addr_filter = create_addr_filter(secret_key.public());

    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(secret_key)
        .alpns(vec![ALPN.to_vec()])
        .addr_filter(addr_filter)
        .bind()
        .await
        .context("bind an endpoint")?;

    let router = Router::new(&config, endpoint).context("Router::new")?;

    router.run().await
}

fn create_addr_filter(id: PublicKey) -> AddrFilter {
    let addr = PeerAddr::from(id);
    AddrFilter::new(move |addrs| Cow::Owned(addrs.iter().filter(|a| match a {
        TransportAddr::Ip(socket) => addr != socket.ip(),
        _ => true
    }).cloned().collect()))
}

fn configure_logging() {
    use tracing_subscriber::EnvFilter;

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("petope=trace".parse().unwrap()))
        .try_init().unwrap();
}