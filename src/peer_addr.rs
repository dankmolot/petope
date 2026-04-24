use crate::{config, utils};
use anyhow::{Context, Result};
use iroh::EndpointId;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone, Copy)]
pub struct PeerAddr {
    pub id: EndpointId,
    pub v4: Ipv4Addr,
    pub v6: Ipv6Addr,
}

impl PeerAddr {
    pub async fn add_route(&self, handle: &net_route::Handle, ifindex: u32) -> Result<()> {
        let route_v4 = net_route::Route::new(IpAddr::V4(self.v4), 32).with_ifindex(ifindex);
        handle
            .add(&route_v4)
            .await
            .with_context(|| format!("add route for {}/32", self.v4))?;

        let route_v6 = net_route::Route::new(IpAddr::V6(self.v6), 128).with_ifindex(ifindex);
        handle
            .add(&route_v6)
            .await
            .with_context(|| format!("add route for {}/128", self.v6))?;

        Ok(())
    }
}

impl From<EndpointId> for PeerAddr {
    fn from(id: EndpointId) -> Self {
        let (addr_v4, addr_v6) = utils::ip_pair_from_id(id);

        PeerAddr {
            id,
            v4: addr_v4,
            v6: addr_v6,
        }
    }
}

impl From<&config::Peer> for PeerAddr {
    fn from(value: &config::Peer) -> Self {
        value.id.into()
    }
}

impl PartialEq<Ipv4Addr> for PeerAddr {
    fn eq(&self, other: &Ipv4Addr) -> bool {
        self.v4.eq(other)
    }
}

impl PartialEq<Ipv6Addr> for PeerAddr {
    fn eq(&self, other: &Ipv6Addr) -> bool {
        self.v6.eq(other)
    }
}

impl PartialEq<IpAddr> for PeerAddr {
    fn eq(&self, other: &IpAddr) -> bool {
        match other {
            IpAddr::V4(addr) => self == addr,
            IpAddr::V6(addr) => self == addr,
        }
    }
}
