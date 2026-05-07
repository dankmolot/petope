use crate::{config, utils};
use arc_swap::ArcSwapOption;
use bytes::{Bytes, BytesMut};
use futures_lite::StreamExt;
use iroh::{
    Endpoint, EndpointId,
    endpoint::{ConnectError, Connection, ConnectionError, VarInt},
};
use ring_channel::{RingReceiver, RingSender};
use std::{
    fmt::Display,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
    time::Instant,
};
use tokio::{io::AsyncReadExt, sync::Notify};

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
                        self.send(&conn, bytes).await;
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
    async fn send(&self, conn: &Connection, bytes: Bytes) {
        let start = Instant::now();
        let Ok(mut send) = conn.open_uni().await else {
            return;
        };

        let id = send.id().index();
        let len = bytes.len();

        tokio::spawn(async move {
            if let Err(e) = send.write_chunk(bytes).await {
                eprintln!("stream {} write err: {:?}", id, e);
                return;
            }

            println!(
                "--> [{}] {} bytes {} ms",
                id,
                len,
                start.elapsed().as_millis()
            );
        });
    }

    // listens for datagrams for stored internally connection,
    // automatically uses latest connection if previous one was closed
    async fn listen(&self) {
        loop {
            while let Some(conn) = self.try_get_connection() {
                while let Ok(recv) = conn.accept_uni().await {
                    let start = Instant::now();
                    let tx = self.to_network_tx.clone();
                    tokio::spawn(async move {
                        const MAX: u16 = std::u16::MAX;
                        let id = recv.id().index();
                        let mut buf = BytesMut::with_capacity(MAX.into());
                        let mut recv = recv.take(MAX.into());

                        while recv.read_buf(&mut buf).await.unwrap() != 0 {}

                        println!(
                            "<-- [{}] {} bytes {} ms",
                            id,
                            buf.len(),
                            start.elapsed().as_millis()
                        );
                        let _ = tx.send(buf.freeze());
                    });
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
