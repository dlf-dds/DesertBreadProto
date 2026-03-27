//! SPIRE/SPIFFE workload identity integration.
//!
//! Validates that peers presenting WireGuard keys have valid SPIRE-issued
//! X.509-SVIDs. This ensures that a stolen iroh keypair alone cannot join
//! the WireGuard overlay without also having valid workload attestation.
//!
//! Phase 2: Currently stubbed — returns Ok for all validations.
//! Full implementation will connect to the local SPIRE Agent Workload API
//! via the Unix domain socket at /tmp/spire-agent/public/api.sock.

use std::path::Path;
use thiserror::Error;
use tracing::{debug, warn};

#[derive(Error, Debug)]
pub enum SpireError {
    #[error("SPIRE agent not available at {path}")]
    AgentNotAvailable { path: String },
    #[error("SVID validation failed for peer {peer_id}: {reason}")]
    SvidInvalid { peer_id: String, reason: String },
    #[error("trust bundle not available for domain {domain}")]
    TrustBundleUnavailable { domain: String },
}

/// SPIRE client that connects to the local SPIRE Agent.
#[derive(Debug, Clone)]
pub struct SpireClient {
    /// Path to the SPIRE Agent Workload API socket
    socket_path: String,
    /// Whether SPIRE validation is enabled
    enabled: bool,
}

impl SpireClient {
    /// Create a new SPIRE client.
    ///
    /// If the socket doesn't exist, validation is disabled (dev mode).
    pub fn new(socket_path: &str) -> Self {
        let enabled = Path::new(socket_path).exists();
        if !enabled {
            debug!(
                socket = socket_path,
                "SPIRE agent socket not found — validation disabled (dev mode)"
            );
        }
        Self {
            socket_path: socket_path.to_string(),
            enabled,
        }
    }

    /// Validate that a peer's iroh EndpointId has a valid SPIRE-issued SVID.
    ///
    /// Phase 2 stub: always returns Ok.
    /// Full implementation will:
    /// 1. Connect to SPIRE Agent Workload API
    /// 2. Fetch the trust bundle for the peer's trust domain
    /// 3. Validate the peer's X.509-SVID against the trust bundle
    /// 4. Check SPIFFE ID matches expected pattern
    pub fn validate_peer(&self, peer_id: &str) -> Result<(), SpireError> {
        if !self.enabled {
            debug!(peer = peer_id, "SPIRE validation skipped (disabled)");
            return Ok(());
        }

        // Phase 2: actual validation
        // For now, log and accept
        warn!(
            peer = peer_id,
            socket = %self.socket_path,
            "SPIRE validation not yet implemented — accepting peer"
        );
        Ok(())
    }

    /// Fetch our own X.509-SVID from the SPIRE Agent.
    ///
    /// Phase 2 stub: returns None.
    pub fn fetch_svid(&self) -> Option<Vec<u8>> {
        if !self.enabled {
            return None;
        }
        warn!("SPIRE SVID fetch not yet implemented");
        None
    }

    /// Check if SPIRE validation is active.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

impl Default for SpireClient {
    fn default() -> Self {
        Self::new("/tmp/spire-agent/public/api.sock")
    }
}
