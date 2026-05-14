use crate::{routing_table::RoutingTable, tun::TunDevice, utils};
use bytes::Bytes;
use etherparse::IpSlice;
use futures::SinkExt;
use ipnetwork::IpNetwork;
use iroh::{
    EndpointId,
    endpoint::{Connection, ConnectionError, SendDatagramError},
};
use log::{debug, error, warn};
use prefix_trie::joint::JointPrefixSet;
use ringbuffer::{AllocRingBuffer, RingBuffer};
use std::{
    fmt,
    sync::{Arc, Mutex, RwLock, Weak},
};
use thiserror::Error;
use tokio::sync::{Notify, futures::Notified, mpsc};

pub type PeerRoutingTable = RoutingTable<Peer>;

pub struct Peer {
    name: RwLock<String>,
    id: EndpointId,
    device: Weak<TunDevice>,
    routing_table: Weak<PeerRoutingTable>,
    routes: Arc<RwLock<JointPrefixSet<IpNetwork>>>,
    connection: RwLock<Option<Connection>>,
    send_queue: Mutex<Option<AllocRingBuffer<Bytes>>>,
    connection_request: mpsc::Sender<()>,
}

impl Peer {
    pub fn new(
        id: EndpointId,
        device: Weak<TunDevice>,
        routing_table: Weak<PeerRoutingTable>,
        connection_request: mpsc::Sender<()>,
    ) -> Self {
        Peer {
            id,
            device,
            routing_table,
            connection_request,
            name: RwLock::new(id.fmt_short().to_string()),
            routes: Arc::default(),
            connection: RwLock::default(),
            send_queue: Mutex::default(),
        }
    }

    pub fn id(&self) -> EndpointId {
        self.id
    }

    pub fn name(&self) -> String {
        self.name.read().unwrap().clone()
    }

    pub fn set_name(&self, new_name: String) {
        *self.name.write().unwrap() = new_name;
    }

    pub fn addresses(&self) -> Vec<IpNetwork> {
        self.routes.read().unwrap().iter().collect()
    }

    // returns current TunDevice associated with the peer or an ErrorKind::NetworkDown error if interface was dropped
    fn get_device(&self) -> std::io::Result<Arc<TunDevice>> {
        use std::io::{Error, ErrorKind};
        self.device.upgrade().ok_or_else(|| {
            Error::new(
                ErrorKind::NetworkDown,
                "tun device is unavailable (was closed?)",
            )
        })
    }

    fn get_routing_table(&self) -> std::io::Result<Arc<PeerRoutingTable>> {
        use std::io::{Error, ErrorKind};
        self.routing_table
            .upgrade()
            .ok_or_else(|| Error::new(ErrorKind::NetworkDown, "routing table is unavailable"))
    }

    // not really thread-safe yet, do not run concurrently
    pub async fn add_prefix(self: &Arc<Self>, prefix: IpNetwork) -> std::io::Result<()> {
        let device = self.get_device()?;
        let routing = device.routing()?;
        let routing_table = self.get_routing_table()?;

        // todo: improve error context
        routing.add(&prefix).await?;
        routing_table.insert(prefix, self.clone());
        self.routes.write().unwrap().insert(prefix);

        Ok(())
    }

    pub fn set_connection(&self, conn: Connection) {
        self.receive_packets(conn.clone());

        if let Some(old) = self.connection.write().unwrap().replace(conn) {
            warn!("old connection {} was replaced for {self}", old.stable_id());
            old.close(0u8.into(), b"you got replaced bro");
        }

        self.send_queued_packets();
    }

    fn receive_packets(&self, conn: Connection) {
        let routes = self.routes.clone();
        let mut writer = match self.device.upgrade() {
            Some(device) => device.writer(),
            None => {
                error!("unable to receive packets from {self} since tun device is unavailable");
                return;
            }
        };

        tokio::spawn(async move {
            while let Ok(bytes) = conn.read_datagram().await {
                // before forwarding to the interface, check if packet has correct source ip
                let packet = match IpSlice::from_slice(&bytes) {
                    Ok(packet) => packet,
                    Err(e) => {
                        error!(
                            "received bad packet ({} bytes) from peer {}: {e}",
                            bytes.len(),
                            conn.remote_id().fmt_short()
                        );
                        continue;
                    }
                };

                let src: IpNetwork = packet.source_addr().into();
                if !routes.read().unwrap().get_lpm(&src).is_some() {
                    warn!(
                        "peer {} sent a packet with source {}, but isn't allowed",
                        conn.remote_id().fmt_short(),
                        src
                    );
                    continue;
                }

                // good, packet is validated and ready to go
                let _ = writer.send(bytes).await;
            }

            if let Some(reason) = conn.close_reason() {
                match reason {
                    ConnectionError::LocallyClosed => {}
                    reason => warn!(
                        "connection with {} was closed due: {reason}",
                        conn.remote_id().fmt_short()
                    ),
                }
            }
        });
    }

    pub fn send(&self, bytes: Bytes) -> Result<(), SendError> {
        if let Some(conn) = self.connection.read().unwrap().clone() {
            return self.handle_send(bytes, conn);
        }

        self.connect_and_send_later(bytes);
        Ok(())
    }

    fn handle_send(&self, bytes: Bytes, conn: Connection) -> Result<(), SendError> {
        if let Err(err) = conn.send_datagram(bytes.clone()) {
            match err {
                SendDatagramError::ConnectionLost(err) => {
                    // remove connection if it wasn't replaced
                    self.connection
                        .write()
                        .unwrap()
                        .take_if(|old| old.stable_id() == conn.stable_id());

                    debug!(
                        "send to {} failed due connection lost, retrying: {err:?}",
                        conn.remote_id().fmt_short()
                    );
                    return self.send(bytes);
                }
                SendDatagramError::TooLarge => {
                    return Err(SendError::TooLarge(
                        conn.max_datagram_size().unwrap_or(0),
                        bytes,
                    ));
                }
                // Some other datagram error, just forward
                err => return Err(err.into()),
            }
        }
        Ok(())
    }

    fn connect_and_send_later(&self, bytes: Bytes) {
        // create a queue if it does not exist and push bytes
        self.send_queue
            .lock()
            .unwrap()
            .get_or_insert_with(|| AllocRingBuffer::new(32))
            .enqueue(bytes);

        let _ = self.connection_request.try_send(());
    }

    fn send_queued_packets(&self) {
        let Some(mut queue) = self.send_queue.lock().unwrap().take() else {
            return;
        };

        let mut too_big = Vec::new();

        debug!("sending {} queued packets to {self}", queue.len());
        while let Some(bytes) = queue.dequeue() {
            if let Err(e) = self.send(bytes) {
                match e {
                    SendError::TooLarge(mtu, bytes) => {
                        if let Some(payload) = utils::fragmentation_needed_response(&bytes, mtu) {
                            too_big.push(payload);
                        }
                    }
                    SendError::Other(err) => error!("send queued to {self} failed: {err}"),
                }
            }
        }

        if !too_big.is_empty() {
            debug!(
                "{} of those queued packets were too big for {self}",
                too_big.len()
            );

            let Some(mut writer) = self.device.upgrade().map(|d| d.writer()) else {
                return;
            };

            tokio::spawn(async move {
                for bytes in too_big {
                    let _ = writer.send(bytes).await;
                }
            });
        }
    }
}

impl fmt::Display for Peer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Peer({:?})", &self.name)
    }
}

#[derive(Error, Debug)]
pub enum SendError {
    #[error("packet is too big, current mtu is {0}")]
    TooLarge(usize, Bytes),

    #[error(transparent)]
    Other(#[from] SendDatagramError),
}
