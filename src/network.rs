use anyhow::{Context, Result};
use dashmap::{DashMap, Entry};
use etherparse::IpSlice;
use futures::{SinkExt, StreamExt};
use ipnetwork::IpNetwork;
use iroh::{Endpoint, EndpointId};
use log::error;
use prefix_trie::joint::JointPrefixMap;
use std::{
    fmt,
    sync::{Arc, Mutex},
};

use crate::{peer::Peer, tun::TunDevice};

pub const ALPN: &[u8] = b"petope/0";

pub struct Network {
    pub name: String,
    endpoint: Endpoint,
    peers: DashMap<EndpointId, Arc<Peer>>,
    device: TunDevice,
    routing_table: Arc<Mutex<JointPrefixMap<IpNetwork, EndpointId>>>,
}

impl Network {
    pub fn new(name: String, endpoint: Endpoint, device: TunDevice) -> Self {
        Network {
            name,
            endpoint,
            peers: DashMap::new(),
            device,
            routing_table: Arc::default(),
        }
    }

    pub fn add_local_addr(&self, addr: IpNetwork) -> std::io::Result<()> {
        // add addr on tun device
        self.device.add_ip(addr)?;
        // route added address to current endpoint id
        self.routing_table
            .lock()
            .unwrap()
            .insert(addr, self.endpoint.id());
        Ok(())
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

    pub fn endpoint(&self) -> Endpoint {
        self.endpoint.clone()
    }

    pub fn run(&self) {
        self.tun_reader();
    }

    fn tun_reader(&self) {
        let my_id = self.endpoint.id();
        let mut reader = self.device.reader();
        let mut writer = self.device.writer();
        let routing_table = self.routing_table.clone();

        tokio::spawn(async move {
            // receive bytes from tun device
            while let Some(Ok(bytes)) = reader.next().await {
                // parse bytes into ip packet
                let packet = match IpSlice::from_slice(&bytes[..]) {
                    Ok(packet) => packet,
                    Err(e) => {
                        error!("received bad packet from tun: {:?}", e);
                        continue;
                    }
                };

                // extract destination addresses
                let dst: IpNetwork = packet.destination_addr().into();

                // get EndpointId by longest matching prefix
                let found_id = routing_table
                    .lock()
                    .unwrap()
                    .get_lpm(&dst)
                    .map(|(_, id)| id.clone());

                if let Some(id) = found_id {
                    // route packet back if id matches current endpoint id
                    if id == my_id {
                        let _ = writer.send(bytes).await;
                        continue;
                    }

                    println!("{} bytes -> {}", bytes.len(), id);
                }
            }
        });
    }
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Network({:?})", &self.name)
    }
}
