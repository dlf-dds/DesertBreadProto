//! Peer state management.
//!
//! Tracks known mesh peers: their iroh identity, tunnel public key,
//! overlay IP, connection status, and last-seen timestamp.
//!
//! Field names use `tunnel_` prefix (not `wg_`) because the mesh protocol is
//! tunnel-agnostic — see [`crate::tunnel::TunnelDriver`] for the design rationale.

use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Wire protocol message exchanged between peers over iroh.
///
/// These fields are tunnel-agnostic: `tunnel_pubkey` and `tunnel_endpoint` are
/// opaque strings whose meaning depends on the active [`TunnelDriver`](crate::tunnel::TunnelDriver).
/// For WireGuard, the pubkey is a base64 Curve25519 key and the endpoint is `ip:port`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerHandshake {
    /// Tunnel public key — opaque to the mesh protocol, interpreted by the tunnel driver
    pub tunnel_pubkey: String,
    /// Overlay IP derived from iroh EndpointId
    pub overlay_ip: Ipv4Addr,
    /// Optional tunnel endpoint (e.g., `ip:port`) for direct connection
    pub tunnel_endpoint: Option<String>,
}

/// Information about a known peer.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub endpoint_id: EndpointId,
    pub tunnel_pubkey: String,
    pub overlay_ip: Ipv4Addr,
    pub tunnel_endpoint: Option<String>,
    pub last_seen: Instant,
    pub connected: bool,
}

/// Thread-safe peer table.
#[derive(Debug, Clone)]
pub struct PeerTable {
    inner: Arc<RwLock<HashMap<EndpointId, PeerInfo>>>,
}

impl PeerTable {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add or update a peer from a handshake message.
    pub async fn upsert(&self, endpoint_id: EndpointId, handshake: PeerHandshake) -> bool {
        let mut table = self.inner.write().await;
        let is_new = !table.contains_key(&endpoint_id);

        let info = PeerInfo {
            endpoint_id,
            tunnel_pubkey: handshake.tunnel_pubkey,
            overlay_ip: handshake.overlay_ip,
            tunnel_endpoint: handshake.tunnel_endpoint,
            last_seen: Instant::now(),
            connected: true,
        };

        if is_new {
            info!(peer = %endpoint_id, ip = %info.overlay_ip, "new peer discovered");
        } else {
            debug!(peer = %endpoint_id, "peer info updated");
        }

        table.insert(endpoint_id, info);
        is_new
    }

    /// Mark a peer as disconnected.
    pub async fn mark_disconnected(&self, endpoint_id: &EndpointId) -> Option<PeerInfo> {
        let mut table = self.inner.write().await;
        if let Some(peer) = table.get_mut(endpoint_id) {
            peer.connected = false;
            info!(peer = %endpoint_id, "peer disconnected");
            Some(peer.clone())
        } else {
            None
        }
    }

    /// Remove a peer entirely.
    pub async fn remove(&self, endpoint_id: &EndpointId) -> Option<PeerInfo> {
        let mut table = self.inner.write().await;
        let removed = table.remove(endpoint_id);
        if let Some(ref peer) = removed {
            info!(peer = %endpoint_id, ip = %peer.overlay_ip, "peer removed");
        }
        removed
    }

    /// Get a snapshot of all peers.
    pub async fn list(&self) -> Vec<PeerInfo> {
        self.inner.read().await.values().cloned().collect()
    }

    /// Get a specific peer's info.
    pub async fn get(&self, endpoint_id: &EndpointId) -> Option<PeerInfo> {
        self.inner.read().await.get(endpoint_id).cloned()
    }

    /// Number of known peers.
    pub async fn count(&self) -> usize {
        self.inner.read().await.len()
    }
}

impl Default for PeerTable {
    fn default() -> Self {
        Self::new()
    }
}
