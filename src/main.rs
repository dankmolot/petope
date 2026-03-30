use clap::{Parser, Subcommand};

/// My experiments with udp connection
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Connect {},
    Discovery {},
}

fn main() {
    let args = Args::parse();

    match &args.command {
        Commands::Connect {} => return connect(args),
        Commands::Discovery {} => return discovery(args),
    }
}

fn connect(args: Args) {
    println!("connect")
}

fn discovery(args: Args) {
    println!("discovery")
}
