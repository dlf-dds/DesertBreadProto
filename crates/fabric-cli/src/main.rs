use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "fabric-cli", about = "Operator CLI for tactical mesh network")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Show mesh status overview
    Status,
    /// List connected peers with details
    Peers,
    /// Show SPIRE certificate status (Phase 2)
    Certs,
    /// Show active Zenoh topics (Phase 3)
    Topics,
    /// Generate a provisioning bundle for a new node
    Provision {
        /// Node hostname
        #[arg(long)]
        hostname: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Command::Status => {
            println!("mesh status: not yet connected to meshd");
            println!("(meshd IPC will be implemented in Phase 1)");
        }
        Command::Peers => {
            println!("peer list: not yet connected to meshd");
        }
        Command::Certs => {
            println!("SPIRE certificate status: Phase 2");
        }
        Command::Topics => {
            println!("Zenoh topic list: Phase 3");
        }
        Command::Provision { hostname } => {
            println!("provision {hostname}: use the `provision` tool directly");
        }
    }

    Ok(())
}
