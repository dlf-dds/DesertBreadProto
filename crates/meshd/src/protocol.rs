//! Mesh protocol handler for iroh connections.
//!
//! Implements the ProtocolHandler trait to accept incoming peer connections,
//! exchange WireGuard keys and overlay IPs, and manage the peer lifecycle.

use crate::peer::{PeerHandshake, PeerTable};
use crate::wireguard::WgInterface;
use anyhow::Result;
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// ALPN protocol identifier for Desert Bread mesh.
pub const MESH_ALPN: &[u8] = b"desert-bread/mesh/0";

/// Protocol handler for mesh peer connections.
#[derive(Debug, Clone)]
pub struct MeshProtocol {
    pub peers: PeerTable,
    pub local_handshake: Arc<RwLock<PeerHandshake>>,
    pub wg: Arc<RwLock<Option<WgInterface>>>,
}

impl MeshProtocol {
    pub fn new(
        peers: PeerTable,
        local_handshake: PeerHandshake,
        wg: Arc<RwLock<Option<WgInterface>>>,
    ) -> Self {
        Self {
            peers,
            local_handshake: Arc::new(RwLock::new(local_handshake)),
            wg,
        }
    }

    /// Initiate a handshake with a remote peer (outbound connection).
    pub async fn handshake_outbound(&self, conn: &Connection) -> Result<()> {
        let remote_id = conn.remote_id();
        debug!(peer = %remote_id, "initiating outbound handshake");

        let (mut send, mut recv) = conn.open_bi().await?;

        // Send our handshake
        let local = self.local_handshake.read().await;
        let msg = postcard::to_allocvec(&*local)?;
        send.write_all(&msg).await?;
        send.finish()?;

        // Receive their handshake
        let response = recv.read_to_end(4096).await?;
        let remote_hs: PeerHandshake = postcard::from_bytes(&response)?;

        // Register peer and configure WireGuard
        let is_new = self.peers.upsert(remote_id, remote_hs.clone()).await;
        if is_new {
            self.configure_wg_peer(&remote_hs).await;
        }

        Ok(())
    }

    /// Configure a WireGuard peer entry from a handshake.
    async fn configure_wg_peer(&self, hs: &PeerHandshake) {
        let wg_guard = self.wg.read().await;
        if let Some(ref wg) = *wg_guard {
            if let Err(e) =
                wg.add_peer(&hs.wg_pubkey, hs.wg_endpoint.as_deref(), hs.overlay_ip)
            {
                warn!(error = %e, peer_wg = %hs.wg_pubkey, "failed to add WireGuard peer");
            }
        }
    }

    /// Remove a WireGuard peer entry.
    pub async fn remove_wg_peer(&self, wg_pubkey: &str) {
        let wg_guard = self.wg.read().await;
        if let Some(ref wg) = *wg_guard {
            if let Err(e) = wg.remove_peer(wg_pubkey) {
                warn!(error = %e, peer_wg = wg_pubkey, "failed to remove WireGuard peer");
            }
        }
    }
}

impl ProtocolHandler for MeshProtocol {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let remote_id = conn.remote_id();
        debug!(peer = %remote_id, "accepting inbound connection");

        let (mut send, mut recv) = conn
            .accept_bi()
            .await
            .map_err(|e| AcceptError::from_err(e))?;

        // Receive their handshake
        let msg = recv
            .read_to_end(4096)
            .await
            .map_err(|e| AcceptError::from_err(e))?;
        let remote_hs: PeerHandshake =
            postcard::from_bytes(&msg).map_err(|e| AcceptError::from_err(e))?;

        // Send our handshake
        let local = self.local_handshake.read().await;
        let response = postcard::to_allocvec(&*local).map_err(|e| AcceptError::from_err(e))?;
        send.write_all(&response)
            .await
            .map_err(|e| AcceptError::from_err(e))?;
        send.finish().map_err(|e| AcceptError::from_err(e))?;

        // Wait for the peer to receive our response before the connection drops
        send.stopped()
            .await
            .ok();

        // Register peer and configure WireGuard
        let is_new = self.peers.upsert(remote_id, remote_hs.clone()).await;
        if is_new {
            self.configure_wg_peer(&remote_hs).await;
        }

        Ok(())
    }
}
