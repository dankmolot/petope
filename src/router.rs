use crate::{
    config::Peer,
    connection_manager::ConnectionManagerCommand,
    utils::{self, Addresses},
};
use bytes::Bytes;
use etherparse::IpSlice;
use ipnetwork::IpNetwork;
use iroh::{
    EndpointId,
    endpoint::{Connection, ConnectionError, SendDatagramError},
};
use log::{debug, error, info, warn};
use prefix_trie::joint::JointPrefixMap;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};
use tokio::sync::mpsc;

pub enum RouterCommand {
    RoutePacket(Bytes),
    PeerPacket((EndpointId, Bytes)),
    NewConnection(Connection),
    ConnectionLost(Connection),
}

pub struct Router {
    chan: mpsc::Receiver<RouterCommand>,
    to_network_tx: mpsc::Sender<Bytes>,
    connection_manager: mpsc::Sender<ConnectionManagerCommand>,
    routes: JointPrefixMap<IpNetwork, Arc<Peer>>,
    connections: HashMap<EndpointId, Connection>,
    peers: HashMap<EndpointId, Arc<Peer>>,
    buffer: VecDeque<(EndpointId, Bytes)>,
}

impl Router {
    pub fn new(
        chan: mpsc::Receiver<RouterCommand>,
        to_network_tx: mpsc::Sender<Bytes>,
        connection_manager: mpsc::Sender<ConnectionManagerCommand>,
    ) -> Self {
        Self {
            chan,
            to_network_tx,
            connection_manager,
            routes: JointPrefixMap::new(),
            connections: HashMap::new(),
            peers: HashMap::new(),
            buffer: VecDeque::new(),
        }
    }

    pub fn add_peer(&mut self, peer: Peer) {
        let peer = Arc::new(peer);
        if let Some(current) = self.peers.get(&peer.id) {
            error!("{current} already exists (tried adding {peer})");
            return;
        }

        self.peers.insert(peer.id, peer.clone());

        for route in &peer.addresses {
            if let Some(old) = self.routes.insert(*route, peer.clone()) {
                error!("{peer} has conflicting route {route} with {old}");
            }
        }
    }

    pub async fn run(mut self) {
        while let Some(command) = self.chan.recv().await {
            match command {
                RouterCommand::RoutePacket(bytes) => self.route_packet(bytes).await,
                RouterCommand::PeerPacket((id, bytes)) => self.peer_packet(id, bytes).await,
                RouterCommand::NewConnection(conn) => self.new_connection(conn).await,
                RouterCommand::ConnectionLost(conn) => self.connection_lost(conn),
            }
        }
    }

    fn get_peer(&self, id: EndpointId) -> Arc<Peer> {
        self.peers
            .get(&id)
            .cloned()
            .unwrap_or_else(|| Arc::new(Peer::from(id)))
    }

    async fn route_packet(&mut self, bytes: Bytes) {
        let ip = match IpSlice::from_slice(&bytes) {
            Ok(ip) => ip,
            Err(e) => {
                error!("received bad packet from tun: {:?}", e);
                return;
            }
        };

        let dst: std::net::IpAddr = ip.destination_addr();
        if let Some(peer) = self.routes.get(&dst.into()).cloned() {
            match self.connections.get(&peer.id).cloned() {
                Some(conn) => self.send_datagram(peer, conn, bytes).await,
                None => self.buffer_and_connect(peer, bytes).await,
            }
        }
    }

    async fn send_datagram(&mut self, peer: Arc<Peer>, conn: Connection, bytes: Bytes) {
        if let Err(e) = conn.send_datagram(bytes.clone()) {
            match e {
                SendDatagramError::TooLarge => {
                    let mtu = conn.max_datagram_size().unwrap_or(1000);
                    if let Some(response) = utils::fragmentation_needed_response(&bytes, mtu) {
                        self.send_to_network(response.freeze()).await;
                    }
                }
                SendDatagramError::ConnectionLost(_) => {
                    self.connection_lost(conn);
                    self.buffer_and_connect(peer, bytes).await;
                }
                e => error!("send to {peer} failed: {e:?}"),
            }
        }
    }

    async fn buffer_and_connect(&mut self, peer: Arc<Peer>, bytes: Bytes) {
        if self.buffer.len() == 32 {
            self.buffer.pop_front();
        }
        self.buffer.push_back((peer.id, bytes));
        let _ = self
            .connection_manager
            .send(ConnectionManagerCommand::RequestConnect(peer.id))
            .await;
    }

    async fn send_to_network(&mut self, bytes: Bytes) {
        if self.to_network_tx.send(bytes).await.is_err() {
            self.chan.close();
        }
    }

    async fn peer_packet(&mut self, id: EndpointId, bytes: Bytes) {
        let peer = self.get_peer(id);
        let ip = match IpSlice::from_slice(&bytes) {
            Ok(ip) => ip,
            Err(e) => {
                error!("bad packet from {peer}: {e:?}");
                return;
            }
        };

        let src = ip.source_addr();
        if !self.routes.get_lpm(&src.into()).is_some() {
            warn!(
                "{peer} sent a packet with source {src} but isn't allowed! allowed={}",
                Addresses(&peer.addresses)
            );
            return;
        }

        self.send_to_network(bytes).await;
    }

    async fn new_connection(&mut self, conn: Connection) {
        let peer = self.get_peer(conn.remote_id());

        if let Some(old) = self.connections.insert(conn.remote_id(), conn.clone()) {
            warn!("connection with {peer} was replaced, some packets may be lost");
            old.close(0u8.into(), b"outdated");
        }

        info!("connected to {peer}!");

        let id = peer.id;
        let mut queue = Vec::with_capacity(self.buffer.len());
        self.buffer.retain(|(target_id, bytes)| {
            if id.eq(target_id) {
                queue.push(bytes.clone());
                false
            } else {
                true
            }
        });

        if !queue.is_empty() {
            debug!("sending {} buffered messages to {peer}", queue.len());

            for bytes in queue {
                if conn.close_reason().is_some() {
                    break;
                }

                self.send_datagram(peer.clone(), conn.clone(), bytes).await
            }
        }
    }

    fn connection_lost(&mut self, conn: Connection) {
        use std::collections::hash_map::Entry;

        // remove connection if it is stored
        match self.connections.entry(conn.remote_id()) {
            Entry::Occupied(entry) if entry.get().stable_id() == conn.stable_id() => {
                entry.remove();

                // print a warning if loss wasn't caused by this program
                let peer = self.get_peer(conn.remote_id());
                match conn.close_reason() {
                    Some(ConnectionError::LocallyClosed) => {}
                    e => warn!("lost connection with {peer}: {e:?}"),
                }
            }
            _ => {}
        };
    }
}
