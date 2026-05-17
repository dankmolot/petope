use bytes::{Bytes, BytesMut};
use std::{net::IpAddr, sync::Arc};
use tokio::sync::mpsc;
use tun_rs::SyncDevice;

use crate::{reader, routing::TunRouting, writer};

#[derive(Clone)]
pub struct TunDevice {
    inner: Arc<SyncDevice>,
}

impl TunDevice {
    pub fn new(device: Arc<SyncDevice>) -> Self {
        Self { inner: device }
    }

    pub fn reader(&self, tx: mpsc::Sender<BytesMut>) {
        reader::run(self.inner.clone(), tx);
    }

    pub fn writer(&self, rx: mpsc::Receiver<Bytes>) {
        writer::run(self.inner.clone(), rx);
    }

    pub fn routing(&self) -> std::io::Result<TunRouting> {
        let ifindex = self.inner.if_index()?;
        TunRouting::new(ifindex)
    }

    pub fn add_local_addr(&self, addr: IpAddr, prefix: u8) -> std::io::Result<()> {
        match addr {
            IpAddr::V4(addr) => self.inner.add_address_v4(addr, prefix),
            IpAddr::V6(addr) => self.inner.add_address_v6(addr, prefix),
        }
    }

    pub fn name(&self) -> std::io::Result<String> {
        self.inner.name()
    }
}
