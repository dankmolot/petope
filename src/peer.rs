use std::{fmt, hash::Hash};

use ip_network::IpNetwork;
use iroh::EndpointId;

use crate::config;

#[derive(Debug, Clone)]
pub struct Peer {
    pub name: String,
    pub id: EndpointId,
    pub addresses: Vec<IpNetwork>,
}

impl From<&config::Peer> for Peer {
    fn from(value: &config::Peer) -> Self {
        let name = value
            .name
            .clone()
            .unwrap_or_else(|| value.id.fmt_short().to_string());

        Peer {
            name,
            id: value.id,
            addresses: value.addresses.to_owned(),
        }
    }
}

impl fmt::Display for Peer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Peer({:?})", &self.name)
    }
}

impl Hash for Peer {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}
