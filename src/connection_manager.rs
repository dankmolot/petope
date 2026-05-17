use crate::router::RouterCommand;
use iroh::{
    Endpoint, EndpointId,
    endpoint::{ConnectOptions, Connection, Incoming, IncomingAddr, ZeroRttStatus},
};
use log::{error, warn};
use std::collections::HashSet;
use tokio::sync::mpsc;

pub const ALPN: &[u8] = b"petope/0";

pub enum ConnectionManagerCommand {
    RequestConnect(EndpointId),
    Incoming(Incoming),
    Accept(Connection),
}

pub struct ConnectionManager {
    chan: mpsc::Receiver<ConnectionManagerCommand>,
    router: mpsc::Sender<RouterCommand>,
    endpoint: Endpoint,
    inner_rx: mpsc::Receiver<ConnectionManagerCommand>,
    inner_tx: mpsc::Sender<ConnectionManagerCommand>,
    allowed_peers: HashSet<EndpointId>,
    pending_connections: HashSet<EndpointId>,
}

impl ConnectionManager {
    pub fn new(
        chan: mpsc::Receiver<ConnectionManagerCommand>,
        router: mpsc::Sender<RouterCommand>,
        endpoint: Endpoint,
    ) -> Self {
        let (inner_tx, inner_rx) = mpsc::channel(1);

        Self {
            chan,
            router,
            endpoint,
            inner_rx,
            inner_tx,
            allowed_peers: HashSet::new(),
            pending_connections: HashSet::new(),
        }
    }

    pub fn allow_peer(&mut self, id: EndpointId) {
        self.allowed_peers.insert(id);
    }

    async fn receive_command(&mut self) -> Option<ConnectionManagerCommand> {
        tokio::select! {
            command = self.chan.recv() => command,
            command = self.inner_rx.recv() => command,
        }
    }

    pub async fn run(mut self) {
        self.accept();
        while let Some(command) = self.receive_command().await {
            match command {
                ConnectionManagerCommand::Incoming(incoming) => self.accept_incoming(incoming),
                ConnectionManagerCommand::Accept(conn) => self.accept_connection(conn),
                ConnectionManagerCommand::RequestConnect(id) => self.connect(id),
            }
        }
    }

    fn accept(&self) {
        let endpoint = self.endpoint.clone();
        let tx = self.inner_tx.clone();
        tokio::spawn(async move {
            while let Some(incoming) = endpoint.accept().await {
                if tx
                    .send(ConnectionManagerCommand::Incoming(incoming))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        });
    }

    fn is_allowed(&self, id: &EndpointId) -> bool {
        self.allowed_peers.contains(id)
    }

    fn accept_incoming(&self, incoming: Incoming) {
        let remote_addr = incoming.remote_addr();
        let remote_id = match remote_addr {
            IncomingAddr::Relay { endpoint_id, .. } => Some(endpoint_id),
            _ => None,
        };

        if let Some(remote_id) = remote_id {
            if !self.is_allowed(&remote_id) {
                warn!(
                    "incoming connection {remote_addr:?} with id {remote_id} is not in the peer list!"
                );
                incoming.refuse();
                return;
            }
        }

        let incoming_zero_rtt = match incoming.accept() {
            Ok(a) => a.into_0rtt(),
            Err(e) => {
                error!("bad incoming connection {remote_addr:?}: {e}");
                return;
            }
        };

        if let Some(remote_id) = incoming_zero_rtt.remote_id().ok() {
            if !self.is_allowed(&remote_id) {
                incoming_zero_rtt.close(403u16.into(), b"not allowed");
                warn!(
                    "incoming 0-RTT connection {remote_addr:?} with id {remote_id} is not in the peer list!"
                );
            }
        }

        let tx = self.inner_tx.clone();
        tokio::spawn(async move {
            match incoming_zero_rtt.handshake_completed().await {
                Ok(conn) => {
                    let _ = tx.send(ConnectionManagerCommand::Accept(conn)).await;
                }
                Err(e) => error!("handshake with {remote_addr:?} failed: {e}"),
            }
        });
    }

    fn accept_connection(&mut self, conn: Connection) {
        if !self.is_allowed(&conn.remote_id()) {
            conn.close(403u16.into(), b"not allowed");
            return;
        }

        let id = conn.remote_id();
        self.pending_connections.remove(&conn.remote_id());

        let router = self.router.clone();
        tokio::spawn(async move {
            // notify router that new connection was accepted
            if router
                .send(RouterCommand::NewConnection(conn.clone()))
                .await
                .is_err()
            {
                return;
            }

            // read data from the peer
            while let Ok(bytes) = conn.read_datagram().await {
                if router
                    .send(RouterCommand::PeerPacket((id, bytes)))
                    .await
                    .is_err()
                {
                    return;
                }
            }

            // read_datagram returned an error, connection must have been lost
            let _ = router.send(RouterCommand::ConnectionLost(conn)).await;
        });
    }

    fn connect(&mut self, id: EndpointId) {
        if self.pending_connections.contains(&id) {
            return;
        }

        self.pending_connections.insert(id);

        let endpoint = self.endpoint.clone();
        let tx = self.inner_tx.clone();
        tokio::spawn(async move {
            let connecting = match endpoint
                .connect_with_opts(id, ALPN, ConnectOptions::new())
                .await
            {
                Ok(conn) => conn,
                Err(e) => {
                    error!("connect to {id} failed: {e:?}");
                    return;
                }
            };

            // try 0-RTT handshake and complete handshake
            let result = match connecting.into_0rtt() {
                Ok(conn) => match conn.handshake_completed().await {
                    Ok(result) => match result {
                        ZeroRttStatus::Accepted(conn) => Ok(conn),
                        ZeroRttStatus::Rejected(conn) => Ok(conn),
                    },
                    Err(e) => Err(e),
                },
                Err(conn) => conn.await,
            };

            match result {
                Ok(conn) => {
                    let _ = tx.send(ConnectionManagerCommand::Accept(conn)).await;
                }
                Err(e) => error!("connect to {id} failed: {e:?}"),
            }
        });
    }
}
