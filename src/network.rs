use anyhow::{Context, Result};
use dashmap::{DashMap, Entry};
use ipnetwork::IpNetwork;
use iroh::{Endpoint, EndpointId};
use std::{fmt, sync::Arc};

use crate::{peer::Peer, tun::TunDevice};

pub const ALPN: &[u8] = b"petope/0";

pub struct Network {
    pub name: String,
    endpoint: Endpoint,
    peers: DashMap<EndpointId, Arc<Peer>>,
    device: TunDevice,
}

impl Network {
    pub fn new(name: String, endpoint: Endpoint, device: TunDevice) -> Self {
        Network {
            name,
            endpoint,
            peers: DashMap::new(),
            device,
        }
    }

    pub fn add_local_addr(&self, addr: IpNetwork) -> std::io::Result<()> {
        self.device.add_ip(addr)
    }

    pub async fn add_peers(&self, peers: impl Iterator<Item = &Arc<Peer>>) -> Result<()> {
        let routing = self.device.routing().context("get tun routing")?;
        for peer in peers {
            match self.peers.entry(peer.id) {
                Entry::Occupied(entry) => {
                    let old = entry.get();
                    let removed = old.addresses.iter().filter(|a| !peer.addresses.contains(a));
                    for addr in removed {
                        routing
                            .remove(addr)
                            .await
                            .with_context(|| format!("delete {} from routing", &addr))?;
                    }
                    let added = peer.addresses.iter().filter(|a| !old.addresses.contains(a));
                    for addr in added {
                        routing
                            .remove(addr)
                            .await
                            .with_context(|| format!("delete {} from routing", &addr))?;
                    }
                    entry.replace_entry(peer.clone());
                }
                Entry::Vacant(entry) => {
                    for addr in &peer.addresses {
                        routing
                            .add(addr)
                            .await
                            .with_context(|| format!("add {} to routing", &addr))?;
                    }

                    entry.insert(peer.clone());
                }
            }
        }

        Ok(())
    }

    pub fn device_name(&self) -> &str {
        &self.device.name
    }

    pub fn peers(&self) -> Vec<Arc<Peer>> {
        self.peers.iter().map(|v| v.value().clone()).collect()
    }

    pub fn local_addrs(&self) -> Vec<IpNetwork> {
        self.device.addresses()
    }

    pub async fn run(&self) {}
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Network({:?})", &self.name)
    }
}
