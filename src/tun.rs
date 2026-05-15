use anyhow::{Context, Result, bail};
use ipnetwork::IpNetwork;
use std::sync::{Arc, Mutex};
use tun_rs::{
    AsyncDevice, DeviceBuilder,
    async_framed::{BytesCodec, DeviceFramedRead, DeviceFramedWrite},
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
