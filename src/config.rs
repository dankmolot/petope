use crate::based_key::BasedKey;
use anyhow::{Context, Result};
use iroh::SecretKey;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    private_key: Option<BasedKey>,
}

impl Config {
    pub fn secret_key_or_generate(&mut self) -> SecretKey {
        self.private_key
            .get_or_insert_with(|| SecretKey::generate().into())
            .clone()
            .into()
    }

    pub fn load(path: &str) -> Config {
        let data = std::fs::read_to_string(path).unwrap_or_default();
        toml::from_str(&data).unwrap()
    }

    pub fn save(&self, path: &str) -> Result<()> {
        let data = toml::to_string_pretty(self).context("serialize config into toml")?;
        std::fs::write(path, &data).with_context(|| format!("write config into the {}", path))
    }
}
