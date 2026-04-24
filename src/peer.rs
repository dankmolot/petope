use crate::peer_addr::PeerAddr;
use anyhow::Result;
use bytes::BytesMut;
use futures::StreamExt;
use iroh::{Endpoint, EndpointId, endpoint::Connection};
use ring_channel::{RingReceiver, RingSender, ring_channel};
use std::sync::Arc;
use tokio::{
    select,
    sync::{mpsc, oneshot},
};

pub struct Peer {
    pub addr: PeerAddr,
    endpoint: Endpoint,
    send_queue: RingSender<BytesMut>,
    route_queue: mpsc::Sender<BytesMut>,
    connect_request: mpsc::Sender<oneshot::Sender<Connection>>,
    accept_connect: mpsc::Sender<Connection>,
}

impl Peer {
    pub async fn handle(
        endpoint: Endpoint,
        id: EndpointId,
        route_queue: mpsc::Sender<BytesMut>,
    ) -> Arc<Peer> {
        let (send_queue, receiver) = ring_channel(1.try_into().unwrap());
        let (connect_request, connector) = mpsc::channel(1);
        let (accept_connect, acceptor) = mpsc::channel(1);

        let peer = Arc::new(Peer {
            addr: id.into(),
            endpoint,
            route_queue,
            send_queue,
            connect_request,
            accept_connect,
        });

        peer.clone().receiver(receiver).await;
        peer.clone().connector(connector, acceptor).await;

        peer
    }

    // Sends bytes to the peer
    pub fn send(&self, bytes: BytesMut) {
        self.send_queue.send(bytes).ok();
    }

    async fn receiver(self: Arc<Self>, mut chan: RingReceiver<BytesMut>) {
        tokio::spawn(async move {
            while let Some(bytes) = chan.next().await {
                match self.get_connection().await {
                    Ok(conn) => {
                        println!("got connection!");
                        println!(
                            "{} bytes must go to peer {}",
                            bytes.len(),
                            self.addr.id.fmt_short()
                        );
                    }
                    _ => {}
                }
            }
        });
    }

    async fn get_connection(&self) -> Result<Connection> {
        let (tx, rx) = oneshot::channel();
        self.connect_request.send(tx).await?;
        Ok(rx.await?)
    }

    async fn connector(
        self: Arc<Self>,
        mut chan: mpsc::Receiver<oneshot::Sender<Connection>>,
        mut acceptor: mpsc::Receiver<Connection>,
    ) {
        tokio::spawn(async move {
            let mut conn: Option<Connection> = None;

            loop {
                select! {
                    c = acceptor.recv() => {
                        conn.replace(c.unwrap());
                    }
                    r = chan.recv() => match r {
                        Some(request) => {
                            self.process_get_connection(&mut conn, request).await;
                        }
                        None => {
                            return;
                        }
                    }
                }
            }
        });
    }

    // Accepts incoming connection which later will be reused
    pub async fn accept(&self, conn: Connection) {
        match self.accept_connect.try_send(conn) {
            Ok(_) => {}
            Err(_) => {
                eprintln!(
                    "unable to save accepted connection for peer {}",
                    self.addr.id.fmt_short()
                )
            }
        }
    }

    async fn process_get_connection(
        &self,
        conn: &mut Option<Connection>,
        request: oneshot::Sender<Connection>,
    ) {
        if let Some(c) = conn {
            request.send(c.clone()).ok();
            return;
        }

        match self.endpoint.connect(self.addr.id, b"petope/1").await {
            Ok(c) => {
                conn.replace(c.clone());
                request.send(c).ok();
            }
            Err(e) => eprintln!(
                "unable connect to peer {}: {:?}",
                self.addr.id.fmt_short(),
                e
            ),
        }
    }
}
