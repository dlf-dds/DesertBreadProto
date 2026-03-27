//! Integration test: LAN peer discovery and handshake.
//!
//! Verifies that two iroh endpoints can connect directly (simulating LAN),
//! exchange WireGuard keys and overlay IPs via the mesh protocol.

use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, RelayMode, SecretKey, TransportAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::timeout;

const MESH_ALPN: &[u8] = b"desert-bread/mesh/0";

/// Derive overlay IP (duplicated here since meshd is a binary crate)
fn overlay_ip(id: &iroh::EndpointId) -> std::net::Ipv4Addr {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(id.as_bytes());
    let raw = u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]]);
    let mut host = raw & 0x003F_FFFF;
    let last = host & 0xFF;
    if last == 0 {
        host |= 1;
    } else if last == 255 {
        host &= !1;
    }
    std::net::Ipv4Addr::from(0x6440_0000 | host)
}

#[tokio::test]
async fn two_nodes_connect_and_handshake() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("warn")
        .try_init();

    let key_a = SecretKey::generate(&mut rand::rng());
    let key_b = SecretKey::generate(&mut rand::rng());

    let id_a = key_a.public();
    let id_b = key_b.public();

    let ip_a = overlay_ip(&id_a);
    let ip_b = overlay_ip(&id_b);

    // Use Minimal preset — no DNS, no relay, just local QUIC
    let ep_a = Endpoint::builder(presets::N0DisableRelay)
        .secret_key(key_a)
        .alpns(vec![MESH_ALPN.to_vec()])
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .expect("bind A");

    let ep_b = Endpoint::builder(presets::N0DisableRelay)
        .secret_key(key_b)
        .alpns(vec![MESH_ALPN.to_vec()])
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .expect("bind B");

    // Get B's bound port — sockets bind to 0.0.0.0 so we use 127.0.0.1
    let b_sockets = ep_b.bound_sockets();
    let b_port = b_sockets
        .iter()
        .find(|s| s.is_ipv4())
        .expect("should have IPv4 socket")
        .port();
    let b_direct: std::net::SocketAddr = format!("127.0.0.1:{b_port}").parse().unwrap();

    let b_received = Arc::new(RwLock::new(false));
    let b_received_clone = b_received.clone();

    // B: accept loop
    let ep_b_clone = ep_b.clone();
    let accept_handle = tokio::spawn(async move {
        if let Some(incoming) = ep_b_clone.accept().await {
            let conn = incoming.await.expect("accept connection");

            let (mut send, mut recv) = conn.accept_bi().await.expect("accept bi");
            let _msg = recv.read_to_end(4096).await.expect("read handshake");

            let response = format!("wg_pubkey_b:{ip_b}");
            send.write_all(response.as_bytes())
                .await
                .expect("write response");
            send.finish().expect("finish send");

            *b_received_clone.write().await = true;

            // Keep connection alive until peer is done reading
            conn.closed().await;
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // A: connect to B using localhost + B's port
    let b_addr = EndpointAddr::from_parts(
        id_b,
        vec![TransportAddr::Ip(b_direct)],
    );

    let conn = timeout(Duration::from_secs(10), ep_a.connect(b_addr, MESH_ALPN))
        .await
        .expect("connect timeout")
        .expect("connect to B");

    assert_eq!(conn.remote_id(), id_b);

    let (mut send, mut recv) = conn.open_bi().await.expect("open bi");

    let handshake = format!("wg_pubkey_a:{ip_a}");
    send.write_all(handshake.as_bytes())
        .await
        .expect("write handshake");
    send.finish().expect("finish");

    let response = recv.read_to_end(4096).await.expect("read response");
    let response_str = String::from_utf8_lossy(&response);
    assert!(
        response_str.contains("wg_pubkey_b"),
        "expected B's WG key in response"
    );

    // Close connection from A's side so B's conn.closed() resolves
    conn.close(0u32.into(), b"done");

    timeout(Duration::from_secs(5), accept_handle)
        .await
        .expect("accept timeout")
        .expect("accept task");

    assert!(*b_received.read().await, "B should have received handshake");

    // Cleanup
    ep_a.close().await;
    ep_b.close().await;
}

#[tokio::test]
async fn overlay_ip_determinism() {
    let key = SecretKey::generate(&mut rand::rng());
    let id = key.public();

    let ip1 = overlay_ip(&id);
    let ip2 = overlay_ip(&id);
    assert_eq!(ip1, ip2);

    let key2 = SecretKey::generate(&mut rand::rng());
    let ip3 = overlay_ip(&key2.public());
    assert_ne!(ip1, ip3);
}

#[tokio::test]
async fn overlay_ip_always_in_cgnat_range() {
    for _ in 0..200 {
        let key = SecretKey::generate(&mut rand::rng());
        let ip = overlay_ip(&key.public());
        let octets = ip.octets();
        assert_eq!(octets[0], 100);
        assert!(octets[1] >= 64 && octets[1] <= 127);
        assert_ne!(octets[3], 0);
        assert_ne!(octets[3], 255);
    }
}
