use anyhow::{Context, Result, bail};
use bytes::{BufMut, Bytes, BytesMut};
use etherparse::IpSlice;
use futures::{SinkExt, StreamExt};
use ipnetwork::IpNetwork;
use log::warn;
use std::{
    borrow::Borrow,
    collections::VecDeque,
    ops::{Deref, Range},
    sync::{Arc, Mutex},
    time::Duration,
};
use tun_rs::{
    AsyncDevice, DeviceBuilder, SyncDevice,
    async_framed::{BytesCodec, DeviceFramed, DeviceFramedRead, DeviceFramedWrite},
};

#[cfg(target_os = "macos")]
static DEVICE_PREFIX: &str = "utun";

#[cfg(not(target_os = "macos"))]
static DEVICE_PREFIX: &str = "petope";

pub struct TunDevice {
    pub name: String,
    pub index: u32,
    device: Arc<AsyncDevice>,
    addresses: Mutex<Vec<IpNetwork>>,
}

impl TunDevice {
    pub async fn create(name: &str, mtu: Option<u16>) -> Result<TunDevice> {
        let device = DeviceBuilder::new()
            .name(name)
            .mtu(mtu.unwrap_or(1500))
            .layer(tun_rs::Layer::L3)
            .with(|_opt| {
                #[cfg(target_os = "linux")]
                _opt.offload(true);
            })
            .build_async()
            .context("build tun device")?;

        let device = Arc::new(device);
        let index = device.if_index().context("get tun device index")?;

        Ok(TunDevice {
            name: name.to_string(),
            index,
            device,
            addresses: Mutex::default(),
        })
    }

    pub fn reader(&self) -> DeviceFramedRead<BytesCodec, Arc<AsyncDevice>> {
        DeviceFramedRead::new(self.device.clone(), BytesCodec::new())
    }

    pub fn writer(&self) -> DeviceFramedWrite<BytesCodec, Arc<AsyncDevice>> {
        DeviceFramedWrite::new(self.device.clone(), BytesCodec::new())
    }

    pub fn add_ip(&self, net: IpNetwork) -> std::io::Result<()> {
        let mut addresses = self.addresses.lock().unwrap();
        if !addresses.contains(&net) {
            match net {
                IpNetwork::V4(net) => self.device.add_address_v4(net.ip(), net.prefix())?,
                IpNetwork::V6(net) => self.device.add_address_v6(net.ip(), net.prefix())?,
            };
            addresses.push(net);
        }

        Ok(())
    }

    pub fn routing<'a>(&'a self) -> std::io::Result<TunRouting<'a>> {
        TunRouting::new(self)
    }

    pub fn addresses(&self) -> Vec<IpNetwork> {
        self.addresses.lock().unwrap().clone()
    }

    pub fn test() {
        let (tx1, rx1) = TunDevice::chan();
        let (tx2, rx2) = TunDevice::chan();

        TunDevice::testdev("testing1", "100.0.0.1", tx2, rx1);
        TunDevice::testdev("testing2", "100.0.0.2", tx1, rx2);
    }

    fn chan() -> (kanal::Sender<Bytes>, kanal::Receiver<Bytes>) {
        kanal::bounded(32)
    }

    fn testdev(name: &str, addr: &str, tx: kanal::Sender<Bytes>, rx: kanal::Receiver<Bytes>) {
        let device = DeviceBuilder::new()
            .name(name)
            .mtu(1500)
            .layer(tun_rs::Layer::L3)
            .ipv4(addr, 32, None)
            .build_sync()
            .unwrap();

        let device = Arc::new(device);

        let reader = TunReader::new(device.clone());
        std::thread::Builder::new()
            .name(format!("read {}", name))
            .spawn(move || {
                TunDevice::recv(reader, tx);
            })
            .unwrap();

        std::thread::Builder::new()
            .name(format!("send {}", name))
            .spawn(move || {
                TunDevice::send(device, rx);
            })
            .unwrap();
    }

    fn recv(mut reader: TunReader<Arc<SyncDevice>>, tx: kanal::Sender<Bytes>) {
        loop {
            let bytes = reader.read().unwrap();
            tx.send(bytes.freeze()).unwrap();
        }
    }

    fn send(dev: Arc<SyncDevice>, rx: kanal::Receiver<Bytes>) {
        loop {
            let bytes = rx.recv().unwrap();
            dev.send(&bytes).unwrap();
        }
    }
}

pub struct TunRouting<'a> {
    handle: net_route::Handle,
    device: &'a TunDevice,
}

impl TunRouting<'_> {
    pub fn new<'a>(device: &'a TunDevice) -> std::io::Result<TunRouting<'a>> {
        let handle = net_route::Handle::new()?;
        Ok(TunRouting { handle, device })
    }

    fn ip_to_route(&self, net: &IpNetwork) -> net_route::Route {
        net_route::Route::new(net.ip(), net.prefix()).with_ifindex(self.device.index)
    }

    pub async fn add(&self, target: &IpNetwork) -> std::io::Result<()> {
        self.handle.add(&self.ip_to_route(target)).await
    }

    pub async fn remove(&self, target: &IpNetwork) -> std::io::Result<()> {
        self.handle.delete(&self.ip_to_route(target)).await
    }
}

// either returns interface name from config or finds first available interface name
pub fn get_device_name_with_prefix(prefix: Option<&str>) -> Result<String> {
    let prefix = prefix.unwrap_or(DEVICE_PREFIX);

    // get all interfaces that start with the prefix
    let interfaces = getifs::interfaces()
        .context("get interfaces")?
        .into_iter()
        .filter(|i| i.name().starts_with(prefix))
        .map(|i| i.name().clone())
        .collect::<Vec<getifs::SmolStr>>();

    for i in 0..100 {
        let name = format!("{}{}", prefix, i);

        // check if none of the interfaces have the name
        if !interfaces.iter().any(|v| v.as_str() == name) {
            return Ok(name);
        }
    }

    bail!(
        "unable to find an available tun device name with prefix {}, already {} interfaces exist",
        prefix,
        interfaces.len()
    );
}

pub fn get_device_name() -> Result<String> {
    get_device_name_with_prefix(None)
}

pub struct TunReader<T> {
    device: T,
    mtu: usize,
    arena: BufferArena,
}

impl<T> TunReader<T>
where
    T: Borrow<SyncDevice>,
{
    pub fn new(device: T) -> Self {
        let mtu = device.borrow().mtu().unwrap_or(4096) as usize;

        Self {
            device,
            mtu,
            arena: BufferArena::new(1, mtu * 16),
        }
    }

    pub fn read(&mut self) -> std::io::Result<BytesMut> {
        let mut buf = self.arena.get_shard(self.mtu);
        let received = self.device.borrow().recv(&mut buf)?;
        buf.truncate(received);

        if received == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "for some reason got 0 bytes",
            ));
        }

        Ok(buf.split())
    }
}

struct BufferArena {
    offset: usize,
    cluster: Vec<BytesMut>,
    shards_batch: usize,
    capacity_per_shard: usize,
}

impl BufferArena {
    pub fn new(shards_batch: usize, capacity_per_shard: usize) -> Self {
        BufferArena {
            offset: 0,
            cluster: Vec::new(),
            capacity_per_shard,
            shards_batch,
        }
    }

    pub fn get_shard<'a>(&'a mut self, minimum_size: usize) -> BytesMut {
        assert!(
            minimum_size <= self.capacity_per_shard,
            "({} > {}) minimum size of a shard exceeds max capacity per shard",
            minimum_size,
            self.capacity_per_shard
        );

        // try to find a suitable shard in the cluster
        if let Some((pos, shard)) = self.find_available_shard(minimum_size) {
            self.offset = pos;
            return shard;
        }

        // put offset at the end, since new buffers will be available
        self.offset = self.cluster.len();

        // no available buffers were found, try increasing cluster size
        let old_capacity = self.cluster.capacity();
        self.cluster.reserve_exact(self.shards_batch);
        warn!(
            "buffer cluster is too small; capacity: previous={old_capacity} allocated={} current={}",
            self.cluster.len(),
            self.cluster.capacity(),
        );

        // since capacity was increased, fill out empty slots with buffers
        for _ in self.cluster.len()..self.cluster.capacity() {
            self.cluster
                .push(BytesMut::with_capacity(self.capacity_per_shard));
        }

        // run recursively since next call will return a buffer guranteed
        self.get_shard(minimum_size)
    }

    fn find_available_shard(&mut self, minimum_size: usize) -> Option<(usize, BytesMut)> {
        let until_end = self.offset..self.cluster.len();
        let until_offset = 0..self.offset;
        self.find_available_shard_by_range(until_end, minimum_size)
            .or_else(|| self.find_available_shard_by_range(until_offset, minimum_size))
    }

    fn find_available_shard_by_range(
        &mut self,
        range: Range<usize>,
        minimum_size: usize,
    ) -> Option<(usize, BytesMut)> {
        for pos in range {
            if let Some(shard) = self.try_get_shard(pos, minimum_size) {
                return Some((pos, shard));
            }
        }
        None
    }

    fn try_get_shard(&mut self, pos: usize, minimum_size: usize) -> Option<BytesMut> {
        if let Some(buffer) = self.cluster.get_mut(pos) {
            if buffer.try_reclaim(minimum_size) {
                unsafe { buffer.set_len(minimum_size) }
                return Some(buffer.split());
            }
        }
        None
    }
}
