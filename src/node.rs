use std::net::SocketAddr;

use anyhow::{Context, Result, bail};
use clap::Args;
use tokio::net::lookup_host;

use crate::{discovery::DiscoveryClient, utils};

#[derive(Args, Debug)]
pub struct NodeArgs {
    id: String,
    target: String,

    // Discovery server address
    #[arg(long, short, default_value_t = String::from("127.0.0.1:4444"))]
    discovery: String,
}

pub async fn main(args: NodeArgs) -> Result<()> {
    discovery_worker(&args.discovery, args.target).await?;
    Ok(())
}

async fn discovery_worker(addr: &str, id: String) -> Result<SocketAddr> {
    let discovery_addr = lookup_host(addr)
        .await?
        .filter(|v| v.is_ipv4()) // no ipv6 support yet :p
        .next()
        .context("unable to lookup discovery address")?;

    let stream = utils::reusable_socket(None)?
        .connect(discovery_addr)
        .await
        .with_context(|| format!("connect to discovery server via {}", discovery_addr))?;
    let local_addr = stream.local_addr()?;

    let mut server = DiscoveryClient::new(stream);

    server.register(id.clone()).await.unwrap();
    match server.get(id.clone()).await.unwrap() {
        Some(public_addr) => {
            println!(
                "registered on {} as \"{}\" with address {}",
                discovery_addr, &id, public_addr
            )
        }
        None => bail!("discovery server {} failed to register us", discovery_addr),
    };

    Ok(local_addr)
}
