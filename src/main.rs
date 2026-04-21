use crate::config::{Config, Peer};
use anyhow::{Context, Result};
use clap::Parser;
use iroh::{Endpoint, endpoint::presets};

mod config;
mod utils;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Path to a config file
    #[arg(short, long, default_value_t = String::from("config.toml"))]
    config: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let (secret_key, config) = Config::load(&cli.config).context("load config")?;

    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(secret_key)
        .bind()
        .await
        .context("bind an endpoint")?;

    println!("running as {}", endpoint.id().to_z32());

    tokio::signal::ctrl_c().await?;
    println!("bye-bye");
    Ok(())
}
