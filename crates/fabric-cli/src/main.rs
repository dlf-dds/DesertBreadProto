use anyhow::Result;
use clap::{Parser, Subcommand};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

const DEFAULT_SOCKET: &str = "/var/run/meshd/meshd.sock";

#[derive(Parser, Debug)]
#[command(name = "fabric-cli", about = "Operator CLI for tactical mesh network")]
struct Args {
    #[command(subcommand)]
    command: Command,

    /// meshd IPC socket path
    #[arg(long, default_value = DEFAULT_SOCKET, global = true)]
    socket: String,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Show mesh status overview
    Status,
    /// List connected peers with details
    Peers,
    /// Show this node's identity
    Identity,
    /// Show SPIRE certificate status (Phase 2)
    Certs,
    /// Show active Zenoh topics (Phase 3)
    Topics,
}

async fn ipc_query(socket_path: &str, cmd: &str) -> Result<Value> {
    let stream = UnixStream::connect(socket_path).await.map_err(|e| {
        anyhow::anyhow!(
            "cannot connect to meshd at {socket_path}: {e}\n\
             Is meshd running? Check: meshd --ipc-socket {socket_path}"
        )
    })?;

    let (reader, mut writer) = stream.into_split();
    let request = format!("{{\"cmd\":\"{cmd}\"}}\n");
    writer.write_all(request.as_bytes()).await?;

    let mut lines = BufReader::new(reader).lines();
    if let Some(line) = lines.next_line().await? {
        let value: Value = serde_json::from_str(&line)?;
        Ok(value)
    } else {
        anyhow::bail!("no response from meshd")
    }
}

fn print_status(v: &Value) {
    println!("Mesh Status");
    println!("===========");
    println!("  Node ID:      {}", v["node_id"].as_str().unwrap_or("?"));
    println!("  Overlay IP:   {}", v["overlay_ip"].as_str().unwrap_or("?"));
    println!("  Peers:        {}", v["peer_count"]);
    println!("  Tunnel:       {}", v["tunnel_interface"].as_str().unwrap_or("?"));
    println!(
        "  SPIRE:        {}",
        if v["spire_enabled"].as_bool().unwrap_or(false) {
            "enabled"
        } else {
            "disabled"
        }
    );
}

fn print_peers(v: &Value) {
    let peers = v["peers"].as_array();
    match peers {
        Some(peers) if !peers.is_empty() => {
            println!("{:<44} {:<18} {:<10} TUNNEL KEY", "NODE ID", "OVERLAY IP", "STATUS");
            println!("{}", "-".repeat(100));
            for p in peers {
                let status = if p["connected"].as_bool().unwrap_or(false) {
                    "online"
                } else {
                    "offline"
                };
                println!(
                    "{:<44} {:<18} {:<10} {}",
                    p["node_id"].as_str().unwrap_or("?"),
                    p["overlay_ip"].as_str().unwrap_or("?"),
                    status,
                    p["tunnel_pubkey"].as_str().unwrap_or("?"),
                );
            }
        }
        _ => println!("No peers connected."),
    }
}

fn print_identity(v: &Value) {
    println!("Node Identity");
    println!("=============");
    println!("  Node ID:    {}", v["node_id"].as_str().unwrap_or("?"));
    println!("  Overlay IP: {}", v["overlay_ip"].as_str().unwrap_or("?"));
    println!("  Tunnel Key: {}", v["tunnel_pubkey"].as_str().unwrap_or("?"));
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Command::Status => {
            let resp = ipc_query(&args.socket, "status").await?;
            print_status(&resp);
        }
        Command::Peers => {
            let resp = ipc_query(&args.socket, "peers").await?;
            print_peers(&resp);
        }
        Command::Identity => {
            let resp = ipc_query(&args.socket, "identity").await?;
            print_identity(&resp);
        }
        Command::Certs => {
            println!("SPIRE certificate status: Phase 2 (not yet implemented)");
        }
        Command::Topics => {
            println!("Zenoh topic list: Phase 3 (not yet implemented)");
        }
    }

    Ok(())
}
