use crate::utils;
use anyhow::{Context, Result};
use ip_network::IpNetwork;
use iroh::{EndpointId, PublicKey, SecretKey};
use log::debug;
use serde::Deserialize;
use toml_edit::{DocumentMut, de::from_document};

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    #[serde(default)]
    pub network: Vec<Network>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Network {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub address: Vec<IpNetwork>,
    #[serde(default)]
    pub peers: Vec<Peer>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Peer {
    pub name: Option<String>,
    pub id: EndpointId,
    #[serde(default)]
    pub addresses: Vec<IpNetwork>,
}

impl Config {
    pub fn load(path: &str) -> Result<(SecretKey, Config)> {
        debug!("loading config from {path}");
        let data = Config::read_file(path).with_context(|| format!("read config from {}", path))?;

        let mut doc = data
            .parse::<DocumentMut>()
            .with_context(|| format!("parse {}", path))?;

        let private_key =
            Config::get_or_generate_secret_key(path, &mut doc).context("get private key")?;

        let public_key = private_key.public();

        Ok((private_key, Config::process(doc, public_key)?))
    }

    fn read_file(path: &str) -> std::io::Result<String> {
        match std::fs::read_to_string(path) {
            Ok(data) => Ok(data),
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Ok("".to_string())
                } else {
                    Err(e)
                }
            }
        }
    }

    fn get_or_generate_secret_key(path: &str, doc: &mut DocumentMut) -> Result<SecretKey> {
        let item: Option<&toml_edit::Item> =
            doc.get("secret_key").or_else(|| doc.get("private_key"));

        if let Some(encoded) = item.and_then(|v| v.as_str()) {
            let decoded =
                utils::base64_decode(encoded).context("private key must be encoded in base64")?;

            decoded
                .as_slice()
                .try_into()
                .context("private key must be valid ed25519 key bytes encoded in base64")
        } else {
            let key = SecretKey::generate();
            doc.insert(
                "private_key",
                utils::base64_encode(&key.clone().to_bytes()).into(),
            );

            std::fs::write(path, doc.to_string())
                .with_context(|| format!("write {} with generated private key", path))?;

            Ok(key)
        }
    }

    fn parse(doc: DocumentMut) -> Result<Self, toml_edit::de::Error> {
        from_document(doc)
    }

    fn process(doc: DocumentMut, id: PublicKey) -> Result<Self> {
        let mut config = Config::parse(doc).context("parse config")?;
        for n in &mut config.network {
            if n.name.is_empty() {
                n.name = "Unnamed".to_string();
            }

            if n.address.is_empty() {
                n.address.push(utils::ipv4_from_id(&id).into());
            }

            for p in &mut n.peers {
                if p.addresses.is_empty() {
                    p.addresses.push(utils::ipv4_from_id(&p.id).into());
                }
            }
        }

        Ok(config)
    }
}
