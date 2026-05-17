use bytes::{Bytes, BytesMut};
use std::sync::Arc;
use tokio::sync::mpsc;
use tun_rs::SyncDevice;

use crate::{reader, writer};

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
}
