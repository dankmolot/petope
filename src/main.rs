use crate::{
    config::Config,
    connection_manager::{ALPN, ConnectionManager},
    router::{Router, RouterCommand},
    utils::Addresses,
};
use anyhow::{Context, Result};
use clap::Parser;
use iroh::{Endpoint, SecretKey, TransportAddr, endpoint::presets, endpoint_info::AddrFilter};
use log::info;
use petope_tun::TunBuilder;
use std::{borrow::Cow, net::IpAddr};
use tokio::sync::mpsc;

mod config;
mod connection_manager;
mod router;
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
        let endpoint = create_network(config, secret_key)
            .await
            .context("create network")?;

        tokio::signal::ctrl_c().await?;
        info!("bye bye");

        endpoint.close().await;

        Ok(())
    })
}

async fn create_network(cfg: Config, secret_key: SecretKey) -> Result<Endpoint> {
    let addr_filter = create_addr_filter(cfg.addresses.iter().map(|a| a.ip()).collect());

    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(secret_key)
        .alpns(vec![ALPN.to_vec()])
        .addr_filter(addr_filter)
        .bind()
        .await
        .context("bind an endpoint")?;

    let device = TunBuilder::new().build().context("build a tun device")?;
    for addr in &cfg.addresses {
        device
            .add_local_addr(addr.ip(), addr.prefix())
            .with_context(|| format!("add {} address to the tun device", addr))?;
    }

    info!(
        "network {} if={} addresses={}",
        &cfg.name,
        device.name()?,
        Addresses(&cfg.addresses),
    );

    let (from_network_tx, mut from_network_rx) = mpsc::channel(8);
    let (to_network_tx, to_network_rx) = mpsc::channel(8);
    let (router_tx, router_rx) = mpsc::channel(8);
    let (manager_tx, manager_rx) = mpsc::channel(8);

    let mut router = Router::new(router_rx, to_network_tx, manager_tx);
    let mut manager = ConnectionManager::new(manager_rx, router_tx.clone(), endpoint.clone());

    let routing = device.routing().context("get routing handle")?;
    for peer in cfg.peers {
        for route in &peer.addresses {
            routing
                .add(route.ip(), route.prefix())
                .await
                .context(format!("add route {route} to {peer}"))?;
        }

        info!(
            " - {peer} id={} addresses={}",
            peer.id.fmt_short(),
            Addresses(&peer.addresses)
        );

        manager.allow_peer(peer.id);
        router.add_peer(peer);
    }

    device.reader(from_network_tx);
    device.writer(to_network_rx);

    tokio::spawn(async move {
        router.run().await;
    });

    tokio::spawn(async move {
        manager.run().await;
    });

    tokio::spawn(async move {
        while let Some(bytes) = from_network_rx.recv().await {
            if router_tx
                .send(RouterCommand::RoutePacket(bytes.freeze()))
                .await
                .is_err()
            {
                return;
            }
        }
    });

    Ok(endpoint)
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

fn create_addr_filter(blocked: Vec<IpAddr>) -> AddrFilter {
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
