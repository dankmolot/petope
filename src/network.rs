use crate::{
    peer::{Peer, PeerRoutingTable, SendError},
    tun::TunDevice,
    utils,
};
use dashmap::DashMap;
use etherparse::IpSlice;
use futures::{SinkExt, StreamExt};
use ipnetwork::IpNetwork;
use iroh::{
    Endpoint, EndpointId,
    endpoint::{IncomingAddr, SendDatagramError},
};
use log::{error, info, warn};
use std::{
    fmt,
    sync::{Arc, Weak},
};
use tokio::sync::mpsc;

pub const ALPN: &[u8] = b"petope/0";

pub struct Network {
    pub name: String,
    endpoint: Endpoint,
    device: Arc<TunDevice>,
    peers: Arc<DashMap<EndpointId, Arc<Peer>>>,
    routing_table: Arc<PeerRoutingTable>,
}

impl Network {
    pub fn new(name: String, endpoint: Endpoint, device: TunDevice) -> Self {
        Network {
            name,
            endpoint,
            device: Arc::new(device),
            peers: Arc::default(),
            routing_table: Arc::new(PeerRoutingTable::new()),
        }
    }

    pub fn add_local_addr(&self, addr: IpNetwork) -> std::io::Result<()> {
        // add addr on tun device
        self.device.add_ip(addr)?;
        Ok(())
    }

    pub fn create_peer(&self, id: EndpointId) -> Arc<Peer> {
        let (connection_request, rx) = mpsc::channel(1);
        let peer = Arc::new(Peer::new(
            id,
            Arc::downgrade(&self.device),
            Arc::downgrade(&self.routing_table),
            connection_request,
        ));

        self.peers.insert(id, peer.clone());
        self.connector(Arc::downgrade(&peer), rx);

        return peer;
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
        self.acceptor();
    }

    fn tun_reader(&self) {
        let mut reader = self.device.reader();
        let mut writer = self.device.writer();
        let routing_table = self.routing_table.clone();

        tokio::spawn(async move {
            // receive bytes from tun device
            while let Some(Ok(bytes)) = reader.next().await {
                let bytes = bytes.freeze();

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

                if let Some(peer) = routing_table.get(&dst) {
                    if let Err(err) = peer.send(bytes) {
                        match err {
                            SendError::TooLarge(mtu, bytes) => {
                                if let Some(payload) =
                                    utils::fragmentation_needed_response(&bytes, mtu)
                                {
                                    let _ = writer.send(payload).await;
                                }
                            }
                            SendError::Other(err) => error!("send to {peer} failed: {err}"),
                        }
                    }
                }
            }
        });
    }

    fn acceptor(&self) {
        let endpoint = self.endpoint.clone();
        let peers = self.peers.clone();

        tokio::spawn(async move {
            while let Some(incoming) = endpoint.accept().await {
                let remote_addr = incoming.remote_addr();
                let remote_id = match remote_addr {
                    IncomingAddr::Relay { endpoint_id, .. } => Some(endpoint_id),
                    _ => None,
                };

                if let Some(remote_id) = remote_id {
                    if !peers.contains_key(&remote_id) {
                        warn!(
                            "incoming connection {remote_addr:?} with id {remote_id} is not in the peer list!"
                        );
                        continue;
                    }
                }

                let incoming_zero_rtt = match incoming.accept() {
                    Ok(a) => a.into_0rtt(),
                    Err(e) => {
                        error!("bad incoming connection {remote_addr:?}: {e}");
                        continue;
                    }
                };

                let remote_id = match incoming_zero_rtt.remote_id() {
                    Ok(id) => id,
                    Err(e) => {
                        error!(
                            "incoming 0-RTT connection {remote_addr:?} has bad endpoint id: {e}",
                        );
                        continue;
                    }
                };

                let peer = match peers.get(&remote_id) {
                    Some(peer) => peer.clone(),
                    None => {
                        warn!(
                            "incoming 0-RTT connection {remote_addr:?} with id {remote_id} is not in the peer list!"
                        );
                        continue;
                    }
                };

                // finish handshake outside of main loop
                tokio::spawn(async move {
                    let conn = match incoming_zero_rtt.handshake_completed().await {
                        Ok(conn) => conn,
                        Err(e) => {
                            error!(
                                "handshake with incoming peer {} and address {remote_addr:?} failed: {e}",
                                remote_id.fmt_short()
                            );
                            return;
                        }
                    };

                    info!(
                        "accepted incoming connection {} from {peer}",
                        conn.stable_id()
                    );

                    peer.set_connection(conn);
                });
            }
        });
    }

    fn connector(&self, peer: Weak<Peer>, mut rx: mpsc::Receiver<()>) {
        let endpoint = self.endpoint.clone();
        tokio::spawn(async move {
            while let Some(_) = rx.recv().await {
                if let Some(peer) = peer.upgrade() {
                    match endpoint.connect(peer.id(), ALPN).await {
                        Ok(conn) => {
                            info!("connected to {peer} with connection {}", conn.stable_id());
                            peer.set_connection(conn);
                        }
                        Err(e) => {
                            error!("connect to {peer} failed: {e}");
                        }
                    }

                    // drain buffered requests
                    while rx.try_recv().is_ok() {}
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
