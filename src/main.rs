use std::{borrow::Cow, net::IpAddr, sync::Arc};

use crate::{
    config::Config,
    network::{ALPN, Network},
    peer::Peer,
    tun::TunDevice,
};
use anyhow::{Context, Result};
use clap::Parser;
use dashmap::DashSet;
use iroh::{Endpoint, SecretKey, TransportAddr, endpoint::presets, endpoint_info::AddrFilter};
use log::info;

mod config;
mod network;
mod peer;
mod tun;
mod utils;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Path to a config file
    #[arg(short, long, default_value_t = String::from("config.toml"))]
    config: String,
}

fn main() -> Result<()> {
    configure_logging();

    let cli = Cli::parse();
    let (secret_key, config) = Config::load(&cli.config).context("load config")?;

    info!("your id: {}", secret_key.public());

    let rt = runtime()?;
    rt.block_on(async move {
        let network = create_network(config, secret_key)
            .await
            .context("create network")?;

        let addrs: Vec<String> = network
            .local_addrs()
            .iter()
            .map(|v| v.to_string())
            .collect();
        info!(
            "{} if={} addresses={:?}",
            &network,
            network.device_name(),
            addrs
        );

        for p in network.peers() {
            let addrs: Vec<String> = p.addresses.iter().map(|v| v.to_string()).collect();
            info!(" - {} id={} addresses={:?}", &p, p.id.fmt_short(), addrs)
        }

        network.run();

        tokio::signal::ctrl_c().await?;
        info!("bye bye");

        network.endpoint().close().await;

        Ok(())
    })
}

async fn create_network(cfg: Config, secret_key: SecretKey) -> Result<Network> {
    let blocked_addrs = Arc::new(DashSet::new());

    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(secret_key)
        .alpns(vec![ALPN.to_vec()])
        .addr_filter(create_addr_filter(blocked_addrs.clone()))
        .bind()
        .await
        .context("bind an endpoint")?;

    let name = tun::get_device_name().context("get device name")?;
    let device = TunDevice::create(&name, None)
        .await
        .context("create device")?;

    let network = Network::new(cfg.name, endpoint, device);

    for addr in cfg.addresses {
        blocked_addrs.insert(addr.ip());
        network.add_local_addr(addr).context("add local addr")?;
    }

    let peers: Vec<Arc<Peer>> = cfg.peers.iter().map(|v| Arc::new(v.into())).collect();
    network.add_peers(peers.iter()).await?;

    Ok(network)
}

fn configure_logging() {
    use tracing_subscriber::EnvFilter;

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("petope=trace".parse().unwrap()),
        )
        .try_init()
        .unwrap();
}

fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
}

fn create_addr_filter(blocked: Arc<DashSet<IpAddr>>) -> AddrFilter {
    AddrFilter::new(move |addrs| {
        Cow::Owned(
            addrs
                .iter()
                .filter(|a| match a {
                    TransportAddr::Ip(a) => !blocked.contains(&a.ip()),
                    _ => true,
                })
                .cloned()
                .collect(),
        )
    })
}
