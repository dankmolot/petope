use clap::{Parser, Subcommand};

pub mod discovery;
pub mod node;

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

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Node(args) => node::main(args),
        Commands::Discovery(args) => discovery::main(args),
    }
}
