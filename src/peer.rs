use crate::{config, utils};
use arc_swap::ArcSwapOption;
use bytes::Bytes;
use etherparse::{IpHeadersSlice, IpSlice};
use futures_lite::StreamExt;
use iroh::{
    Endpoint, EndpointId,
    endpoint::{ConnectError, Connection, ConnectionError, SendDatagramError, VarInt},
};
use ring_channel::{RingReceiver, RingSender};
use std::{
    fmt::Display,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
};
use tokio::sync::Notify;

pub const ALPN: &[u8] = b"petope/0";

pub struct Peer {
    pub id: EndpointId,
    pub ipv4: Ipv4Addr,
    pub ipv6: Ipv6Addr,
    endpoint: Endpoint,
    conn: ArcSwapOption<Connection>,
    on_connection_change: Notify,
    to_network_tx: RingSender<Bytes>,
    to_peer_rx: RingReceiver<Bytes>,
}

impl Peer {
    pub fn new(
        config: &config::Peer,
        endpoint: Endpoint,
        to_network_tx: RingSender<Bytes>,
        to_peer_rx: RingReceiver<Bytes>,
    ) -> Arc<Self> {
        let id = config.id;

        Arc::new(Peer {
            id,
            ipv4: utils::ipv4_from_id(&id),
            ipv6: utils::ipv6_from_id(&id),
            endpoint,
            conn: ArcSwapOption::empty(),
            on_connection_change: Notify::new(),
            to_network_tx,
            to_peer_rx,
        })
    }

    pub async fn run(self: &Arc<Peer>) {
        self.clone().forward().await;
        self.listen().await;
    }

    // forwards bytes from `to_peer_rx` to the peer, and automatically connects to it
    async fn forward(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut to_peer_rx = self.to_peer_rx.clone();
            while let Some(bytes) = to_peer_rx.next().await {
                // todo: cache connection reference to prevent unnecessary clones
                match self.get_connection().await {
                    Ok(conn) => {
                        if let Err(e) = self.send(&conn, bytes) {
                            eprintln!("send to {} error: {:?}", self, e);
                        }
                    }
                    Err(e) => {
                        let dropped = utils::drain(&mut to_peer_rx).await + 1; // drop existing packet since connection failed
                        eprintln!(
                            "connect to {} error (dropped {} packets): {:?}",
                            self, dropped, e
                        );
                    }
                }
            }
        });
    }

    // sends a datagram or drops it, and handles TooLarge error
    fn send(&self, conn: &Connection, bytes: Bytes) -> Result<(), SendDatagramError> {
        if let Some(max) = conn.max_datagram_size() {
            if bytes.len() > max {
                self.handle_too_big(bytes, max);
                return Ok(());
            }
        }

        conn.send_datagram(bytes)
    }

    // handles too big packet either by PMTU or by sending fragmented packet
    fn handle_too_big(&self, bytes: Bytes, max: usize) {
        let ip = match IpSlice::from_slice(&bytes[..]) {
            Ok(ip) => ip,
            Err(e) => {
                eprintln!("send to {} bad packet: {:?}", self, e);
                return;
            }
        };

        let header = ip.header();

        let dont_fragment = match header {
            IpHeadersSlice::Ipv4(h, _) => h.dont_fragment(),
            _ => true, // ipv6 routers don't fragment packets
        };

        if dont_fragment {
            let buf = utils::fragmentation_needed_response(&ip, &bytes, max);
            let _ = self.to_network_tx.send(buf.freeze());
        } else {
            eprintln!(
                "todo: send fragmented packet to {} {:?} (len: {} max: {})",
                ip.destination_addr(),
                ip.payload_ip_number().keyword_str(),
                bytes.len(),
                max
            );
        }
    }

    // listens for datagrams for stored internally connection,
    // automatically uses latest connection if previous one was closed
    async fn listen(&self) {
        loop {
            while let Some(conn) = self.try_get_connection() {
                while let Ok(bytes) = conn.read_datagram().await {
                    match IpSlice::from_slice(&bytes[..]) {
                        Ok(_) => {
                            // todo ip filtering
                            let _ = self.to_network_tx.send(bytes).is_err();
                        }
                        Err(e) => eprintln!("bad packet from {}: {:?}", self, e),
                    }
                }

                // usually when `read_datagram` fails, it means that connection has failed
                if let Some(err) = conn.close_reason() {
                    match err {
                        ConnectionError::LocallyClosed => {}
                        e => eprintln!("connection error with {}: {:?}", self, e),
                    }
                } else {
                    eprintln!(
                        "connection to {} for some reason failed without a reason",
                        self
                    );
                }
            }

            // there is no connections right now, wait until one is available
            self.on_connection_change.notified().await;
            // todo: shutdown
        }
    }

    // stores given connection internally, and closes old one
    pub fn accept(&self, conn: Arc<Connection>) {
        if let Some(old) = self.conn.swap(Some(conn)) {
            old.close(VarInt::from_u32(0), b"outdated");
        }

        self.on_connection_change.notify_one();
    }

    // gets current non-closed connection or tries to make a connection
    pub async fn get_connection(&self) -> Result<Arc<Connection>, ConnectError> {
        if let Some(conn) = self.try_get_connection() {
            return Ok(conn);
        }

        match self.endpoint.connect(self.id, ALPN).await {
            Ok(conn) => {
                let conn = Arc::new(conn);
                self.accept(conn.clone());
                Ok(conn)
            }
            Err(e) => Err(e),
        }
    }

    // gets current non-closed connection
    pub fn try_get_connection(&self) -> Option<Arc<Connection>> {
        self.conn
            .load_full()
            .filter(|conn| conn.close_reason().is_none())
    }
}

impl PartialEq<Ipv4Addr> for Peer {
    fn eq(&self, other: &Ipv4Addr) -> bool {
        self.ipv4.eq(other)
    }
}

impl PartialEq<Ipv6Addr> for Peer {
    fn eq(&self, other: &Ipv6Addr) -> bool {
        self.ipv6.eq(other)
    }
}

impl PartialEq<IpAddr> for Peer {
    fn eq(&self, other: &IpAddr) -> bool {
        match other {
            IpAddr::V4(addr) => self == addr,
            IpAddr::V6(addr) => self == addr,
        }
    }
}

impl Display for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Peer({})", &self.id.to_z32()[..8])
    }
}
