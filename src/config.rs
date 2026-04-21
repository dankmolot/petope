use std::str::FromStr;

use crate::utils;
use anyhow::{Context, Result, anyhow};
use iroh::{EndpointId, PublicKey, SecretKey};
use toml_edit::{DocumentMut, Item, Table};

#[derive(Debug, Clone)]
pub struct Config {
    pub peers: Vec<Peer>,
}

#[derive(Debug, Clone)]
pub struct Peer {
    pub id: EndpointId,
}

impl Config {
    pub fn load(path: &str) -> Result<(SecretKey, Config)> {
        let data = Config::read_file(path).with_context(|| format!("read config from {}", path))?;

        let mut doc = data
            .parse::<DocumentMut>()
            .with_context(|| format!("parse {}", path))?;

        let private_key =
            Config::get_or_generate_secret_key(path, &mut doc).context("get private key")?;

        Ok((private_key, Config::parse(doc)?))
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
        if let Some(encoded) = doc.get("private_key").and_then(|v| v.as_str()) {
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

    fn parse(doc: DocumentMut) -> Result<Self> {
        let mut peers = Vec::new();
        if let Some(arr) = doc.get("peer").and_then(Item::as_array_of_tables) {
            for (i, t) in arr.iter().enumerate() {
                peers.push(
                    Peer::parse(t).with_context(|| format!("parse a [[peer]] at index {}", i))?,
                );
            }
        }

        Ok(Config { peers })
    }
}

impl Peer {
    fn parse(t: &Table) -> Result<Self> {
        let id = t
            .get("id")
            .and_then(Item::as_str)
            .ok_or_else(|| anyhow!("expected an `id` but got nothing"))?;

        let id = PublicKey::from_z32(id).context("parse `id` as an EndpointId")?;

        Ok(Peer { id })
    }
}
