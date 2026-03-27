mod bundle;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "provision", about = "Node provisioning for tactical mesh network")]
struct Args {
    #[command(subcommand)]
    command: Command,

    /// Log level
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Generate a node provisioning bundle
    Generate {
        /// Node hostname
        #[arg(long)]
        hostname: String,

        /// Node role (cp, mft, relay)
        #[arg(long)]
        role: String,

        /// Target platform (x86_64, aarch64)
        #[arg(long, default_value = "aarch64")]
        platform: String,

        /// Output directory for the bundle
        #[arg(long, default_value = "./bundles")]
        output: String,

        /// Site name (e.g., alpha, bravo)
        #[arg(long)]
        site: String,
    },
    /// List known provisioning bundles
    List {
        /// Bundle directory
        #[arg(long, default_value = "./bundles")]
        dir: String,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| args.log_level.parse().unwrap()),
        )
        .init();

    match args.command {
        Command::Generate {
            hostname,
            role,
            platform,
            output,
            site,
        } => {
            info!(
                hostname = %hostname,
                role = %role,
                platform = %platform,
                site = %site,
                "generating provisioning bundle"
            );
            bundle::generate(&hostname, &role, &platform, &output, &site)?;
        }
        Command::List { dir } => {
            bundle::list(&dir)?;
        }
    }

    Ok(())
}
