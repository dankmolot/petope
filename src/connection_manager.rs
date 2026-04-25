use bytes::Bytes;
use dashmap::{DashMap, Entry};
use iroh::{
    Endpoint, EndpointId,
    endpoint::{Connection, ConnectionError},
};
use tokio::sync::{mpsc, watch};

pub const ALPN: &[u8] = b"petope/0";

pub struct ConnectionManager {
    endpoint: Endpoint,
    map: DashMap<EndpointId, Connection>,
    queue: DashMap<EndpointId, watch::Receiver<Option<Connection>>>,
    route_tx: mpsc::Sender<Bytes>,
}

impl ConnectionManager {
    pub fn new(endpoint: Endpoint, route_tx: mpsc::Sender<Bytes>) -> Self {
        ConnectionManager {
            map: DashMap::new(),
            queue: DashMap::new(),
            endpoint,
            route_tx,
        }
    }

    pub async fn run(&self) {
        // receive incoming connections and store them
        while let Some(incoming) = self.endpoint.accept().await {
            match incoming.await {
                Ok(conn) => {
                    self.set_connection(conn.clone());
                    Self::handle_connection(conn, self.route_tx.clone());
                }
                Err(e) => {
                    eprintln!("incoming connection failed: {:?}", e);
                }
            }
        }
    }

    // Retrieves existing connection for EndpointId, or tries to connect
    pub async fn get(&self, id: EndpointId) -> Option<Connection> {
        // first return existing connection if it exist
        if let Some(conn) = self.get_connection(&id) {
            return Some(conn);
        }

        // we want a connection to each endpoint id to be queued, so first request a place in queue
        let (mut rx, tx) = self.enqueue(id);

        let initiator = tx.is_some();

        // if sender was returned, then we manage this queue, so we should initiate the connection
        if let Some(tx) = tx {
            // check if connection appeared while we were getting a place in queue
            match self.get_connection(&id) {
                Some(conn) => {
                    // there is existing connection, just broadcast it
                    let _ = tx.send(Some(conn));
                }
                None => {
                    // initiate a connection
                    let endpoint = self.endpoint.clone();
                    tokio::spawn(async move { Self::connect(endpoint, id, tx) });
                }
            }
        }

        rx.changed().await.unwrap();
        let conn = rx.borrow().clone();
        if let Some(conn) = &conn {
            self.set_connection(conn.clone());
            Self::handle_connection(conn.clone(), self.route_tx.clone()).await;
        }

        // only remove from queue after connection is stored!
        if initiator {
            self.queue.remove(&id);
        }

        conn
    }

    async fn connect(
        endpoint: Endpoint,
        addr: EndpointId,
        sender: watch::Sender<Option<Connection>>,
    ) {
        tokio::spawn(async move {
            println!("connecting to peer {}...", addr.fmt_short());
            let result = match endpoint.connect(addr, ALPN).await {
                Ok(conn) => Some(conn),
                Err(e) => {
                    eprintln!("connect to peer {} failed: {:?}", addr.fmt_short(), e);
                    None
                }
            };

            let _ = sender.send(result);
        });
    }

    // returns a receiver for the queue, and optional sender if queue wasn't registered before
    fn enqueue(
        &self,
        id: EndpointId,
    ) -> (
        watch::Receiver<Option<Connection>>,
        Option<watch::Sender<Option<Connection>>>,
    ) {
        match self.queue.entry(id) {
            Entry::Vacant(e) => {
                let (tx, rx) = watch::channel(None);
                e.insert(rx.clone());

                (rx, Some(tx))
            }
            Entry::Occupied(e) => (e.get().clone(), None),
        }
    }

    // replaces old connection with provided one, closes old if exist
    fn set_connection(&self, conn: Connection) {
        if let Some(old) = self.map.insert(conn.remote_id(), conn) {
            old.close(0u8.into(), b"outdated");
        }
    }

    fn get_connection(&self, id: &EndpointId) -> Option<Connection> {
        self.map.get(id).map(|v| v.clone())
    }

    async fn handle_connection(conn: Connection, route_tx: mpsc::Sender<Bytes>) {
        tokio::spawn(async move {
            loop {
                match conn.read_datagram().await {
                    Ok(bytes) => {
                        // todo: filter bad ip packets
                        let _ = route_tx.send(bytes).await;
                    }
                    Err(e) => {
                        match e {
                            ConnectionError::LocallyClosed => {}
                            e => eprintln!(
                                "connection error with {}: {:?}",
                                conn.remote_id().fmt_short(),
                                e
                            ),
                        };
                        break;
                    }
                }
            }
        });
    }
}
