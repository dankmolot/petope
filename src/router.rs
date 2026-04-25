use crate::{config::Config, peer_addr::PeerAddr, tun::TunDevice};
use anyhow::{Context, Result};
use bytes::Bytes;
use etherparse::IpSlice;
use futures::StreamExt;
use iroh::Endpoint;
use ring_channel::{RingReceiver, RingSender, ring_channel};
use std::num::NonZeroUsize;

pub struct Router {
    me: PeerAddr,
    device: TunDevice,
    from_network_rx: RingReceiver<Bytes>,
    to_network_tx: RingSender<Bytes>,
}

impl Router {
    pub fn new(config: &Config, endpoint: Endpoint) -> Result<Self> {
        let me = PeerAddr::from(endpoint.id());
        let (from_network_tx, from_network_rx) = ring_channel(NonZeroUsize::new(8).unwrap());
        let (to_network_tx, to_network_rx) = ring_channel::<Bytes>(NonZeroUsize::new(8).unwrap());

        let device = TunDevice::new(config, &endpoint.id(), from_network_tx, to_network_rx)
            .context("TunDevice::new")?;

        Ok(Router {
            me,
            device,
            from_network_rx,
            to_network_tx,
        })
    }

    pub async fn run(mut self) -> Result<()> {
        tokio::spawn(async move {
            if let Err(e) = self.device.run().await {
                eprintln!("tun device fatal error: {:?}", e);
            }
        });

        tokio::spawn(async move {
            while let Some(bytes) = self.from_network_rx.next().await {
                if let Err(e) = Self::route(bytes, self.me, &self.to_network_tx) {
                    eprintln!("routing error: {:?}", e);
                }
            }
        });

        println!("current id: {}", self.me.id.to_z32());
        println!("ipv4: {} ipv6: {}", self.me.v4, self.me.v6);

        tokio::signal::ctrl_c().await?;
        println!("bye-bye");
        Ok(())
    }

    fn route(bytes: Bytes, me: PeerAddr, to_network_tx: &RingSender<Bytes>) -> Result<()> {
        let ip = IpSlice::from_slice(&bytes[..]).context("parse incoming ip packet")?;
        let dst = ip.destination_addr();

        if me == dst {
            let _ = to_network_tx.send(bytes);
            return Ok(());
        }

        Ok(())
    }
}
