//! Integration test: Island mode — 3-node site with no internet.
//!
//! Simulates a complete site deployment:
//! - 1 CP node + 2 MFT nodes
//! - No relay (island mode)
//! - All nodes discover each other and exchange keys
//! - Verifies full mesh connectivity: every node knows every other node

use iroh::endpoint::presets;
use iroh::protocol::Router;
use iroh::{Endpoint, EndpointAddr, RelayMode, SecretKey, TransportAddr};
use meshd::overlay_ip::overlay_ip_from_id;
use meshd::peer::{PeerHandshake, PeerTable};
use meshd::protocol::{MeshProtocol, MESH_ALPN};
use std::time::Duration;
use tokio::time::timeout;

struct TestNode {
    endpoint: Endpoint,
    _router: Router,
    protocol: MeshProtocol,
    id: iroh::EndpointId,
    overlay_ip: std::net::Ipv4Addr,
    local_port: u16,
}

async fn spawn_node(name: &str) -> TestNode {
    let key = SecretKey::generate(&mut rand::rng());
    let id = key.public();
    let overlay_ip = overlay_ip_from_id(&id);

    let endpoint = Endpoint::builder(presets::N0DisableRelay)
        .secret_key(key)
        .alpns(vec![MESH_ALPN.to_vec()])
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .unwrap_or_else(|e| panic!("{name}: bind failed: {e}"));

    let local_port = endpoint
        .bound_sockets()
        .iter()
        .find(|s| s.is_ipv4())
        .expect("ipv4 socket")
        .port();

    let peers = PeerTable::new();
    let handshake = PeerHandshake {
        tunnel_pubkey: format!("tunnel-{name}"),
        overlay_ip,
        tunnel_endpoint: None,
    };

    let protocol = MeshProtocol::new(peers.clone(), handshake, None);

    let router = Router::builder(endpoint.clone())
        .accept(MESH_ALPN, protocol.clone())
        .spawn();

    TestNode {
        endpoint,
        _router: router,
        protocol,
        id,
        overlay_ip,
        local_port,
    }
}

async fn handshake_pair(initiator: &TestNode, responder: &TestNode) {
    let addr = EndpointAddr::from_parts(
        responder.id,
        vec![TransportAddr::Ip(
            format!("127.0.0.1:{}", responder.local_port)
                .parse()
                .unwrap(),
        )],
    );

    let conn = timeout(
        Duration::from_secs(10),
        initiator.endpoint.connect(addr, MESH_ALPN),
    )
    .await
    .expect("connect timeout")
    .expect("connect failed");

    initiator
        .protocol
        .handshake_outbound(&conn)
        .await
        .expect("handshake failed");

    // Give the responder time to process before closing
    tokio::time::sleep(Duration::from_millis(100)).await;
    conn.close(0u32.into(), b"done");
}

#[tokio::test]
async fn three_node_island_mode() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("warn")
        .try_init();

    // Spawn 3 nodes: 1 CP + 2 MFTs
    let cp = spawn_node("cp").await;
    let mft1 = spawn_node("mft-01").await;
    let mft2 = spawn_node("mft-02").await;

    // All IPs should be unique and in CGNAT range
    assert_ne!(cp.overlay_ip, mft1.overlay_ip);
    assert_ne!(cp.overlay_ip, mft2.overlay_ip);
    assert_ne!(mft1.overlay_ip, mft2.overlay_ip);

    // Simulate LAN discovery: each node connects to the others
    // CP initiates to both MFTs
    handshake_pair(&cp, &mft1).await;
    handshake_pair(&cp, &mft2).await;
    // MFT1 initiates to MFT2
    handshake_pair(&mft1, &mft2).await;

    // Verify full mesh: each node knows the other two
    let cp_peers = cp.protocol.peers.list().await;
    let mft1_peers = mft1.protocol.peers.list().await;
    let mft2_peers = mft2.protocol.peers.list().await;

    assert_eq!(cp_peers.len(), 2, "CP should know 2 peers");
    assert_eq!(mft1_peers.len(), 2, "MFT-01 should know 2 peers");
    assert_eq!(mft2_peers.len(), 2, "MFT-02 should know 2 peers");

    // Verify CP knows both MFTs
    let cp_peer_ids: Vec<_> = cp_peers.iter().map(|p| p.endpoint_id).collect();
    assert!(cp_peer_ids.contains(&mft1.id), "CP should know MFT-01");
    assert!(cp_peer_ids.contains(&mft2.id), "CP should know MFT-02");

    // Verify MFT-01 knows CP and MFT-02
    let mft1_peer_ids: Vec<_> = mft1_peers.iter().map(|p| p.endpoint_id).collect();
    assert!(mft1_peer_ids.contains(&cp.id), "MFT-01 should know CP");
    assert!(mft1_peer_ids.contains(&mft2.id), "MFT-01 should know MFT-02");

    // Verify all peers report as connected
    for p in &cp_peers {
        assert!(p.connected, "all CP peers should be connected");
    }
    for p in &mft1_peers {
        assert!(p.connected, "all MFT-01 peers should be connected");
    }

    // Cleanup
    cp.endpoint.close().await;
    mft1.endpoint.close().await;
    mft2.endpoint.close().await;
}

#[tokio::test]
async fn peer_disconnect_detection() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("warn")
        .try_init();

    let node_a = spawn_node("a").await;
    let node_b = spawn_node("b").await;

    // A connects to B
    handshake_pair(&node_a, &node_b).await;

    // Both should know each other
    assert_eq!(node_a.protocol.peers.count().await, 1);
    assert_eq!(node_b.protocol.peers.count().await, 1);

    // Simulate B going offline: mark disconnected
    node_a
        .protocol
        .peers
        .mark_disconnected(&node_b.id)
        .await;

    let peers = node_a.protocol.peers.list().await;
    assert_eq!(peers.len(), 1);
    assert!(!peers[0].connected, "peer should be marked disconnected");

    // Cleanup
    node_a.endpoint.close().await;
    node_b.endpoint.close().await;
}
