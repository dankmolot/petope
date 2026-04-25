use crate::{config, utils};
use iroh::EndpointId;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone, Copy)]
pub struct PeerAddr {
    pub id: EndpointId,
    pub v4: Ipv4Addr,
    pub v6: Ipv6Addr,
}

impl From<EndpointId> for PeerAddr {
    fn from(id: EndpointId) -> Self {
        PeerAddr {
            id,
            v4: utils::ipv4_from_id(&id),
            v6: utils::ipv6_from_id(&id),
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
