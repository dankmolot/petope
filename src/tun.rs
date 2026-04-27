use crate::{config::Config, utils};
use anyhow::{Context, Result, anyhow, bail};
use bytes::Bytes;
use futures::SinkExt;
use futures_lite::StreamExt;
use iroh::EndpointId;
use ring_channel::{RingReceiver, RingSender};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
};
use tun_rs::{
    AsyncDevice, DeviceBuilder,
    async_framed::{BytesCodec, DeviceFramed},
};

#[cfg(target_os = "macos")]
static DEVICE_PREFIX: &str = "utun";

#[cfg(not(target_os = "macos"))]
static DEVICE_PREFIX: &str = "petope";

pub struct TunDevice {
    pub name: String,
    pub mtu: u16,
    pub ipv4: Ipv4Addr,
    pub ipv6: Ipv6Addr,
    // additional addresses that are routed to the device
    pub routes: Vec<IpAddr>,

    from_network_tx: RingSender<Bytes>,
    to_network_rx: RingReceiver<Bytes>,
}

impl TunDevice {
    pub fn new(
        config: &Config,
        id: &EndpointId,
        from_network_tx: RingSender<Bytes>,
        to_network_rx: RingReceiver<Bytes>,
    ) -> Result<Self> {
        let mut routes = Vec::with_capacity(config.peers.len());
        for p in &config.peers {
            routes.push(IpAddr::V4(utils::ipv4_from_id(&p.id)));
            routes.push(IpAddr::V6(utils::ipv6_from_id(&p.id)));
        }

        Ok(TunDevice {
            name: Self::get_device_name(config).context("get device name")?,
            mtu: config.mtu,
            ipv4: utils::ipv4_from_id(id),
            ipv6: utils::ipv6_from_id(id),
            routes,
            from_network_tx,
            to_network_rx,
        })
    }

    pub async fn handle(mut self) -> Result<()> {
        let dev = self.create_device().context("create tun device")?;
        let ifindex = Self::get_ifindex(&self).context("get interface index")?;

        self.setup_routes(ifindex).await.context("setup routes")?;

        let (mut rx, mut tx) = DeviceFramed::new(Arc::new(dev), BytesCodec::new()).split();

        tokio::spawn(async move {
            while let Some(frame) = rx.next().await {
                match frame {
                    Ok(bytes) => {
                        if let Err(_) = self.from_network_tx.send(bytes.freeze()) {
                            // shutdown
                            return;
                        }
                    }
                    Err(e) => eprintln!("parse frame from tun: {:?}", e),
                }
            }
        });

        tokio::spawn(async move {
            while let Some(bytes) = self.to_network_rx.next().await {
                if let Err(e) = tx.send(bytes).await {
                    eprintln!("parse bytes to frame to tun: {:?}", e);
                }
            }
        });

        Ok(())
    }

    async fn setup_routes(&self, ifindex: u32) -> Result<()> {
        let handle = net_route::Handle::new().context("get routing handle")?;
        for ip in &self.routes {
            let prefix = match ip {
                IpAddr::V4(_) => 32,
                IpAddr::V6(_) => 128,
            };
            let route = net_route::Route::new(*ip, prefix).with_ifindex(ifindex);
            handle
                .add(&route)
                .await
                .with_context(|| format!("add route to {}/{}", ip, prefix))?;
        }

        Ok(())
    }

    fn create_device(&self) -> std::io::Result<AsyncDevice> {
        DeviceBuilder::new()
            .name(&self.name)
            .mtu(self.mtu)
            .ipv4(self.ipv4, 32, None)
            .ipv6(self.ipv6, 128)
            .layer(tun_rs::Layer::L3)
            .with(|_opt| {
                #[cfg(target_os = "macos")]
                _opt.associate_route(false);
            })
            .build_async()
    }

    // getifs crate uses more efficient calls than tun_rs
    fn get_ifindex(&self) -> Result<u32> {
        getifs::interface_by_name(&self.name)
            .context("get interface by name")
            .and_then(|v| v.ok_or_else(|| anyhow!("interface {} not found", &self.name)))
            .map(|i| i.index())
    }

    // either returns interface name from config or finds first available interface name
    fn get_device_name(config: &Config) -> Result<String> {
        if let Some(name) = &config.interface_name {
            return Ok(name.clone());
        }

        let prefix = DEVICE_PREFIX;

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
}
