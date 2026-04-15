use anyhow::Result;
use clap::{Parser, Subcommand};

pub mod connection;
pub mod discovery;
pub mod node;
pub mod utils;

/// My experiments with udp connection
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Node(node::NodeArgs),
    Discovery(discovery::DiscoveryArgs),
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Node(args) => node::main(args).await?,
        Commands::Discovery(args) => discovery::main(args).await?,
    }

    Ok(())
}
