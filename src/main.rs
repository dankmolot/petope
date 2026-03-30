use clap::{Args, Parser, Subcommand};

mod discovery;

/// My experiments with udp connection
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Connect(ConnectArgs),
    Discovery(discovery::DiscoveryArgs),
}

#[derive(Args, Debug)]
struct ConnectArgs {
    target: String,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Connect(args) => connect(args),
        Commands::Discovery(args) => discovery::main(args),
    }
}

fn connect(args: ConnectArgs) {
    println!("connect");
    dbg!(args);
}
