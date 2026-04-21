use crate::utils;
use anyhow::{Context, Result};
use iroh::{PublicKey, SecretKey};
use serde::{Deserialize, Serialize};
use toml_edit::DocumentMut;

#[derive(Debug)]
pub struct Config {
    pub peers: Option<Vec<Peer>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Peer {
    pub public_key: PublicKey,
}

impl Config {
    pub fn load(path: &str) -> Result<(SecretKey, Config)> {
        let data = Config::read_file(path).with_context(|| format!("read config from {}", path))?;

        let mut doc = data
            .parse::<DocumentMut>()
            .with_context(|| format!("parse {}", path))?;

        let private_key =
            Config::get_or_generate_secret_key(path, &mut doc).context("get private key")?;

        Ok((private_key, Config { peers: None }))
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
}
