use crate::{config::Config, peer::Peer, peer_addr::PeerAddr, tun};
use anyhow::{Context, Result};
use bytes::BytesMut;
use etherparse::IpSlice;
use iroh::Endpoint;
use std::{collections::HashMap, net::IpAddr, sync::Arc};
use tokio::sync::mpsc;

pub struct Router {
    pub me: PeerAddr,
    pub peers: Vec<Arc<Peer>>,

    endpoint: Endpoint,
    route_queue: mpsc::Sender<BytesMut>,
    send_queue: mpsc::Sender<BytesMut>,
    peer_routing_table: HashMap<IpAddr, Arc<Peer>>,
}

impl Router {
    pub async fn run(config: &Config, endpoint: Endpoint) -> Result<Arc<Self>> {
        let me: PeerAddr = endpoint.id().into();

        let (route_queue, incoming) = mpsc::channel(128);
        let (send_queue, outcoming) = mpsc::channel(128);
        let ifindex =
            tun::create_tun(config, (me.v4, me.v6), route_queue.clone(), outcoming).await?;

        let mut peers = Vec::with_capacity(config.peers.len());
        for c in &config.peers {
            let p = Peer::handle(c.id, route_queue.clone()).await;
            peers.push(p);
        }

        Router::setup_routes(peers.iter().map(|v| &v.addr), ifindex)
            .await
            .context("setup routes")?;

        let mut peer_routing_table = HashMap::new();
        for peer in &peers {
            peer_routing_table.insert(peer.addr.v4.into(), peer.clone());
            peer_routing_table.insert(peer.addr.v6.into(), peer.clone());
        }

        let router = Arc::new(Router {
            me,
            peers,
            peer_routing_table,
            route_queue,
            send_queue,
            endpoint,
        });

        router.clone().receive(incoming).await;

        Ok(router)
    }

    // runs on background a worker that receives bytes and routes them
    async fn receive(self: Arc<Self>, mut receiver: mpsc::Receiver<BytesMut>) {
        tokio::spawn(async move {
            while let Some(bytes) = receiver.recv().await {
                self.route(bytes)
                    .await
                    .unwrap_or_else(|e| eprintln!("unable to route a packet: {:?}", e));
            }
        });
    }

    async fn route(&self, bytes: BytesMut) -> Result<()> {
        let ip = IpSlice::from_slice(&bytes)?;
        let dst = ip.destination_addr();

        if self.route_queue.capacity() < 4 {
            println!(
                "too much incoming, channel capacity {}/{}",
                self.route_queue.capacity(),
                self.route_queue.max_capacity()
            );
        }

        if self.me == dst {
            self.send_queue.send(bytes).await?;
            return Ok(());
        }

        if let Some(peer) = self.peer_routing_table.get(&dst) {
            peer.send(bytes);
        }

        Ok(())
    }

    async fn setup_routes(peers: impl Iterator<Item = &PeerAddr>, ifindex: u32) -> Result<()> {
        let handle = net_route::Handle::new()?;
        for p in peers {
            p.add_route(&handle, ifindex)
                .await
                .with_context(|| format!("add route for {} peer", p.id.fmt_short()))?;
        }

        Ok(())
    }
}
