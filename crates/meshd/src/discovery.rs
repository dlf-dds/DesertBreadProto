//! Peer discovery via iroh mDNS and relay.
//!
//! Watches for new peers discovered on the LAN (mDNS) or via relay,
//! initiates handshakes, and manages the peer lifecycle.

use crate::protocol::MeshProtocol;
use anyhow::Result;
use iroh::Endpoint;
use iroh::address_lookup::{DiscoveryEvent, MdnsAddressLookup};
use n0_future::StreamExt;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::protocol::MESH_ALPN;

/// Run the mDNS discovery loop.
///
/// Subscribes to mDNS events and initiates handshakes with newly discovered peers.
pub async fn run_mdns_discovery(
    endpoint: Arc<Endpoint>,
    protocol: MeshProtocol,
) -> Result<()> {
    let mdns = MdnsAddressLookup::builder()
        .service_name("desert-bread")
        .build(endpoint.id())?;

    // Register with endpoint's address lookup system
    if let Ok(lookup) = endpoint.address_lookup() {
        lookup.add(mdns.clone());
    }

    let mut events = mdns.subscribe().await;
    info!("mDNS discovery started");

    while let Some(event) = events.next().await {
        match event {
            DiscoveryEvent::Discovered {
                endpoint_info, ..
            } => {
                let peer_id = endpoint_info.endpoint_id;
                let our_id = endpoint.id();

                // Only the peer with the "lower" ID initiates to avoid duplicate connections
                if our_id.as_bytes() >= peer_id.as_bytes() {
                    debug!(peer = %peer_id, "skipping handshake (other side initiates)");
                    continue;
                }

                info!(peer = %peer_id, "mDNS: discovered peer, initiating handshake");

                let ep = endpoint.clone();
                let proto = protocol.clone();
                tokio::spawn(async move {
                    match ep.connect(peer_id, MESH_ALPN).await {
                        Ok(conn) => {
                            if let Err(e) = proto.handshake_outbound(&conn).await {
                                warn!(peer = %peer_id, error = %e, "outbound handshake failed");
                            }
                        }
                        Err(e) => {
                            warn!(peer = %peer_id, error = %e, "failed to connect to discovered peer");
                        }
                    }
                });
            }
            DiscoveryEvent::Expired { endpoint_id } => {
                info!(peer = %endpoint_id, "mDNS: peer expired");
                if let Some(peer_info) = protocol.peers.mark_disconnected(&endpoint_id).await {
                    protocol.remove_tunnel_peer(&peer_info.tunnel_pubkey).await;
                }
            }
        }
    }

    Ok(())
}
