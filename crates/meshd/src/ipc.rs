//! Unix domain socket IPC server for meshd.
//!
//! Provides a local API for fabric-cli and other tools to query
//! mesh status without needing to be part of the mesh themselves.
//!
//! Protocol: newline-delimited JSON over Unix socket.
//! Socket path: /var/run/meshd/meshd.sock (configurable)

use crate::peer::PeerTable;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tracing::info;

/// Default IPC socket path.
pub const DEFAULT_SOCKET_PATH: &str = "/var/run/meshd/meshd.sock";

/// IPC request from a client.
#[derive(Debug, Deserialize)]
#[serde(tag = "cmd")]
pub enum IpcRequest {
    /// Get mesh status overview
    #[serde(rename = "status")]
    Status,
    /// List all peers
    #[serde(rename = "peers")]
    Peers,
    /// Get this node's identity info
    #[serde(rename = "identity")]
    Identity,
}

/// IPC response to a client.
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum IpcResponse {
    #[serde(rename = "status")]
    Status {
        node_id: String,
        overlay_ip: String,
        peer_count: usize,
        tunnel_interface: String,
        spire_enabled: bool,
    },
    #[serde(rename = "peers")]
    Peers { peers: Vec<IpcPeerInfo> },
    #[serde(rename = "identity")]
    Identity {
        node_id: String,
        overlay_ip: String,
        tunnel_pubkey: String,
    },
    #[serde(rename = "error")]
    Error { message: String },
}

#[derive(Debug, Serialize)]
pub struct IpcPeerInfo {
    pub node_id: String,
    pub overlay_ip: String,
    pub tunnel_pubkey: String,
    pub connected: bool,
}

/// Node identity info for IPC responses.
#[derive(Debug, Clone)]
pub struct NodeIdentity {
    pub node_id: String,
    pub overlay_ip: String,
    pub tunnel_pubkey: String,
    pub tunnel_interface: String,
    pub spire_enabled: bool,
}

/// Run the IPC server.
pub async fn run_ipc_server(
    socket_path: &str,
    peers: PeerTable,
    identity: NodeIdentity,
) -> Result<()> {
    // Clean up stale socket
    let path = Path::new(socket_path);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let listener = UnixListener::bind(path)?;
    info!(path = socket_path, "IPC server listening");

    loop {
        let (stream, _) = listener.accept().await?;
        let peers = peers.clone();
        let identity = identity.clone();

        tokio::spawn(async move {
            let (reader, mut writer) = stream.into_split();
            let mut lines = BufReader::new(reader).lines();

            while let Ok(Some(line)) = lines.next_line().await {
                let response = match serde_json::from_str::<IpcRequest>(&line) {
                    Ok(req) => handle_request(req, &peers, &identity).await,
                    Err(e) => IpcResponse::Error {
                        message: format!("invalid request: {e}"),
                    },
                };

                let mut json = serde_json::to_string(&response).unwrap_or_default();
                json.push('\n');

                if writer.write_all(json.as_bytes()).await.is_err() {
                    break;
                }
            }
        });
    }
}

async fn handle_request(
    req: IpcRequest,
    peers: &PeerTable,
    identity: &NodeIdentity,
) -> IpcResponse {
    match req {
        IpcRequest::Status => {
            let count = peers.count().await;
            IpcResponse::Status {
                node_id: identity.node_id.clone(),
                overlay_ip: identity.overlay_ip.clone(),
                peer_count: count,
                tunnel_interface: identity.tunnel_interface.clone(),
                spire_enabled: identity.spire_enabled,
            }
        }
        IpcRequest::Peers => {
            let peer_list = peers.list().await;
            let peers = peer_list
                .into_iter()
                .map(|p| IpcPeerInfo {
                    node_id: p.endpoint_id.to_string(),
                    overlay_ip: p.overlay_ip.to_string(),
                    tunnel_pubkey: p.tunnel_pubkey,
                    connected: p.connected,
                })
                .collect();
            IpcResponse::Peers { peers }
        }
        IpcRequest::Identity => IpcResponse::Identity {
            node_id: identity.node_id.clone(),
            overlay_ip: identity.overlay_ip.clone(),
            tunnel_pubkey: identity.tunnel_pubkey.clone(),
        },
    }
}
