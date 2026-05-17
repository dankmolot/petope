use bytes::Bytes;
use std::sync::Arc;
use tokio::sync::mpsc;
use tun_rs::SyncDevice;

pub fn run(device: Arc<SyncDevice>, rx: mpsc::Receiver<Bytes>) {
    let name = device.name().unwrap_or_else(|_| "tun".to_string());

    std::thread::Builder::new()
        .name(format!("{}-writer", name))
        .spawn(move || writer(device, rx))
        .unwrap();
}

fn writer(device: Arc<SyncDevice>, mut rx: mpsc::Receiver<Bytes>) {
    while let Some(bytes) = rx.blocking_recv() {
        device.send(&bytes).unwrap();
    }
}
