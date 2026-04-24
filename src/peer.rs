use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use iroh::EndpointId;

use crate::{config, utils};

#[derive(Debug, Clone, Copy)]
pub struct Peer {
    pub id: EndpointId,
    pub addr_v4: Ipv4Addr,
    pub addr_v6: Ipv6Addr,
}

impl From<EndpointId> for Peer {
    fn from(id: EndpointId) -> Self {
        let (addr_v4, addr_v6) = utils::ip_pair_from_id(id);

        Peer {
            id,
            addr_v4,
            addr_v6,
        }
    }
}

impl From<&config::Peer> for Peer {
    fn from(value: &config::Peer) -> Self {
        value.id.into()
    }
}

impl PartialEq<Ipv4Addr> for Peer {
    fn eq(&self, other: &Ipv4Addr) -> bool {
        self.addr_v4.eq(other)
    }
}

impl PartialEq<Ipv6Addr> for Peer {
    fn eq(&self, other: &Ipv6Addr) -> bool {
        self.addr_v6.eq(other)
    }
}

impl PartialEq<IpAddr> for Peer {
    fn eq(&self, other: &IpAddr) -> bool {
        match other {
            IpAddr::V4(addr) => self == addr,
            IpAddr::V6(addr) => self == addr,
        }
    }
}
