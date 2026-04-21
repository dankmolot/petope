use crate::config::Config;
use anyhow::Result;
use clap::Parser;

mod based_key;
mod config;

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
    let mut config = Config::load(&cli.config);
    let secret_key = config.secret_key_or_generate();
    config.save(&cli.config)?;

    println!("{}", secret_key.public().fmt_short());

    tokio::signal::ctrl_c().await?;
    println!("bye-bye");
    Ok(())
}
