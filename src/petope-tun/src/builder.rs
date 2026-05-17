use std::sync::Arc;

use tun_rs::DeviceBuilder;

use crate::TunDevice;

#[cfg(target_os = "macos")]
static DEVICE_PREFIX: &str = "utun";

#[cfg(not(target_os = "macos"))]
static DEVICE_PREFIX: &str = "petope";

pub struct TunBuilder(DeviceBuilder);

impl TunBuilder {
    pub fn new() -> Self {
        TunBuilder(DeviceBuilder::new().mtu(1500).layer(tun_rs::Layer::L3))
    }

    pub fn name<S: Into<String>>(self, name: S) -> Self {
        TunBuilder(self.0.name(name))
    }

    pub fn auto_name(self) -> std::io::Result<Self> {
        let name = get_device_name()?;
        Ok(self.name(name))
    }

    pub fn build(self) -> std::io::Result<TunDevice> {
        Ok(TunDevice::new(Arc::new(self.0.build_sync()?)))
    }
}

// either returns interface name from config or finds first available interface name
pub fn get_device_name_with_prefix(prefix: Option<&str>) -> std::io::Result<String> {
    let prefix = prefix.unwrap_or(DEVICE_PREFIX);

    // get all interfaces that start with the prefix
    let interfaces = getifs::interfaces()?
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

    Err(std::io::ErrorKind::QuotaExceeded.into())
}

pub fn get_device_name() -> std::io::Result<String> {
    get_device_name_with_prefix(None)
}
