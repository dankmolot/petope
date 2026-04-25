use crate::{config::Config, peer::Peer, peer_addr::PeerAddr, tun::TunDevice};
use anyhow::{Context, Result};
use bytes::Bytes;
use etherparse::IpSlice;
use futures_lite::StreamExt;
use iroh::{Endpoint, EndpointId};
use ring_channel::{RingReceiver, RingSender, ring_channel};
use std::{collections::HashMap, net::IpAddr, num::NonZeroUsize, sync::Arc};

pub struct Router {
    me: PeerAddr,
    endpoint: Endpoint,
    device: TunDevice,
    peers: HashMap<EndpointId, Arc<Peer>>,
    from_network_rx: RingReceiver<Bytes>,
    to_network_tx: RingSender<Bytes>,
    to_peer_tx_map: HashMap<IpAddr, RingSender<Bytes>>,
}

impl Router {
    pub fn new(config: &Config, endpoint: Endpoint) -> Result<Self> {
        let me = PeerAddr::from(endpoint.id());
        let (from_network_tx, from_network_rx) = ring_channel(NonZeroUsize::new(128).unwrap());
        let (to_network_tx, to_network_rx) = ring_channel::<Bytes>(NonZeroUsize::new(128).unwrap());

        let device = TunDevice::new(config, &endpoint.id(), from_network_tx, to_network_rx)
            .context("TunDevice::new")?;

        let mut peers = HashMap::new();
        let mut to_peer_tx_map = HashMap::new();
        for c in &config.peers {
            let (to_peer_tx, to_peer_rx) = ring_channel(NonZeroUsize::new(128).unwrap());
            let peer = Peer::new(c, endpoint.clone(), to_network_tx.clone(), to_peer_rx);
            to_peer_tx_map.insert(IpAddr::V4(peer.ipv4), to_peer_tx.clone());
            to_peer_tx_map.insert(IpAddr::V6(peer.ipv6), to_peer_tx.clone());
            peers.insert(peer.id, peer);
        }

        println!("---");
        println!("current id: {}", me.id.to_z32());
        println!("ipv4: {} ipv6: {}", me.v4, me.v6);
        println!("---");
        println!("peers:");
        for p in peers.values() {
            println!(" - {} ipv4: {} ipv6: {}", p, p.ipv4, p.ipv6);
        }
        println!("---");

        Ok(Router {
            me,
            endpoint,
            device,
            peers,
            from_network_rx,
            to_network_tx,
            to_peer_tx_map,
        })
    }

    pub async fn run(mut self) -> Result<()> {
        // handles tun i/o
        self.device.handle().await.context("handle tun device")?;

        // handle peers
        for peer in self.peers.values().cloned() {
            tokio::spawn(async move {
                peer.run().await;
            });
        }

        // accept incoming connections
        tokio::spawn(async move {
            while let Some(incoming) = self.endpoint.accept().await {
                match incoming.await {
                    Ok(conn) => {
                        if let Some(peer) = self.peers.get(&conn.remote_id()) {
                            peer.accept(Arc::new(conn));
                        }
                    }
                    Err(e) => eprintln!("incoming conn failed: {:?}", e),
                }
            }
        });

        tokio::spawn(async move {
            while let Some(bytes) = self.from_network_rx.next().await {
                if let Err(e) =
                    Self::route(bytes, self.me, &self.to_network_tx, &self.to_peer_tx_map)
                {
                    eprintln!("routing error: {:?}", e);
                }
            }
        });

        tokio::signal::ctrl_c().await?;
        println!("bye-bye");
        Ok(())
    }

    fn route(
        bytes: Bytes,
        me: PeerAddr,
        to_network_tx: &RingSender<Bytes>,
        to_peer_tx_map: &HashMap<IpAddr, RingSender<Bytes>>,
    ) -> Result<()> {
        let ip = IpSlice::from_slice(&bytes[..]).context("parse incoming ip packet")?;
        let dst = ip.destination_addr();

        if me == dst {
            let _ = to_network_tx.send(bytes);
            return Ok(());
        }

        if let Some(target) = to_peer_tx_map.get(&dst) {
            let _ = target.send(bytes);
        }

        Ok(())
    }
}
