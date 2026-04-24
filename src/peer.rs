use std::sync::Arc;

use crate::peer_addr::PeerAddr;
use bytes::BytesMut;
use futures::StreamExt;
use iroh::EndpointId;
use ring_channel::{RingReceiver, RingSender, ring_channel};
use tokio::sync::mpsc;

pub struct Peer {
    pub addr: PeerAddr,
    send_queue: RingSender<BytesMut>,
    route_queue: mpsc::Sender<BytesMut>,
}

impl Peer {
    pub async fn handle(id: EndpointId, route_queue: mpsc::Sender<BytesMut>) -> Arc<Peer> {
        let (send_queue, receiver) = ring_channel(1.try_into().unwrap());

        let peer = Arc::new(Peer {
            addr: id.into(),
            route_queue,
            send_queue,
        });

        peer.clone().receiver(receiver).await;

        peer
    }

    // Sends bytes to the peer
    pub fn send(&self, bytes: BytesMut) {
        self.send_queue.send(bytes).ok();
    }

    async fn receiver(self: Arc<Self>, mut chan: RingReceiver<BytesMut>) {
        tokio::spawn(async move {
            while let Some(bytes) = chan.next().await {
                println!(
                    "{} bytes must go to peer {}",
                    bytes.len(),
                    self.addr.id.fmt_short()
                );
            }
        });
    }
}
