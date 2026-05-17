use bytes::BytesMut;
use log::debug;
use std::{ops::Range, sync::Arc};
use tokio::sync::mpsc;
use tun_rs::SyncDevice;

pub fn run(device: Arc<SyncDevice>, tx: mpsc::Sender<BytesMut>) {
    let name = device.name().unwrap_or_else(|_| "tun".to_string());

    std::thread::Builder::new()
        .name(format!("{}-reader", name))
        .spawn(move || reader(device, tx))
        .unwrap();
}

fn reader(device: Arc<SyncDevice>, tx: mpsc::Sender<BytesMut>) {
    let mtu = device.mtu().unwrap_or(4096) as usize;
    let mut arena = BufferArena::new(4, mtu * 8);

    loop {
        let mut buf = arena.get_shard(mtu);
        let received = device.recv(&mut buf).unwrap();
        buf.truncate(received);

        if tx.blocking_send(buf).is_err() {
            return;
        }
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

        if !self.cluster.is_empty() {
            debug!(
                "buffer cluster is too small; capacity: previous={old_capacity} allocated={} current={} bytes={}KB",
                self.cluster.len(),
                self.cluster.capacity(),
                self.capacity_per_shard * self.cluster.capacity() / 1000
            );
        }

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
