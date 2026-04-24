use crate::{config::Config, peer::Peer, tun};
use anyhow::{Context, Result};
use bytes::BytesMut;
use etherparse::IpSlice;
use iroh::Endpoint;
use std::{collections::HashMap, net::IpAddr, sync::Arc};
use tokio::sync::mpsc;

pub struct Router {
    pub me: Peer,
    pub peers: Vec<Peer>,
    pub peer_routing_table: HashMap<IpAddr, Peer>,

    endpoint: Endpoint,
    route_queue: mpsc::Sender<BytesMut>,
    send_queue: mpsc::Sender<BytesMut>,
}

impl Router {
    pub async fn run(config: &Config, endpoint: Endpoint) -> Result<Arc<Self>> {
        let me: Peer = endpoint.id().into();

        let (route_queue, incoming) = mpsc::channel(128);
        let (send_queue, outcoming) = mpsc::channel(128);
        let ifindex = tun::create_tun(
            config,
            (me.addr_v4, me.addr_v6),
            route_queue.clone(),
            outcoming,
        )
        .await?;

        let peers: Vec<Peer> = config.peers.iter().map(Peer::from).collect();
        Router::setup_routes(peers.iter(), ifindex)
            .await
            .context("setup routes")?;

        let mut peer_routing_table = HashMap::new();
        for peer in &peers {
            peer_routing_table.insert(peer.addr_v4.into(), *peer);
            peer_routing_table.insert(peer.addr_v6.into(), *peer);
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

        let Some(peer) = self.peer_routing_table.get(&dst) else {
            // drop packets that are not in our routing table
            return Ok(());
        };

        println!("{:?} to peer {}", ip.payload_ip_number(), peer.id);

        Ok(())
    }

    async fn setup_routes(peers: impl Iterator<Item = &Peer>, ifindex: u32) -> Result<()> {
        let handle = net_route::Handle::new()?;
        for p in peers {
            let route_v4 = net_route::Route::new(IpAddr::V4(p.addr_v4), 32).with_ifindex(ifindex);
            handle
                .add(&route_v4)
                .await
                .with_context(|| format!("add route for {}/32", p.addr_v4))?;

            let route_v6 = net_route::Route::new(IpAddr::V6(p.addr_v6), 128).with_ifindex(ifindex);
            handle
                .add(&route_v6)
                .await
                .with_context(|| format!("add route for {}/128", p.addr_v6))?;
        }

        Ok(())
    }
}
