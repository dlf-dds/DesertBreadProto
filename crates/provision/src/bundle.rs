//! Node provisioning bundle generation.
//!
//! Generates everything a node needs to join the mesh:
//! - iroh keypair (permanent node identity)
//! - Overlay IP (deterministic from iroh pubkey)
//! - SPIRE join token (one-time use, for initial attestation)
//! - Known peers list (for LAN bootstrap)
//! - Relay URLs (for cross-NAT connectivity)
//! - Site-specific config (Zenoh topics, NORM multicast groups)

use anyhow::Result;
use iroh::SecretKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::net::Ipv4Addr;
use std::path::Path;
use tracing::info;

/// A complete provisioning bundle for a single node.
#[derive(Debug, Serialize, Deserialize)]
pub struct NodeBundle {
    /// Bundle format version
    pub version: u32,
    /// Node hostname
    pub hostname: String,
    /// Node role: cp, mft, relay
    pub role: String,
    /// Target platform: x86_64, aarch64
    pub platform: String,
    /// Site name: alpha, bravo, etc.
    pub site: String,

    // --- Identity ---
    /// iroh secret key bytes (32 bytes)
    pub iroh_secret_key: Vec<u8>,
    /// iroh public key (hex string, this is the permanent node ID)
    pub iroh_public_key: String,
    /// Deterministic overlay IP in 100.64.0.0/10
    pub overlay_ip: Ipv4Addr,

    // --- SPIRE ---
    /// SPIRE join token for initial attestation (one-time use)
    pub spire_join_token: Option<String>,
    /// SPIRE trust domain for this site
    pub spire_trust_domain: String,
    /// SPIRE server address (overlay IP of the CP node)
    pub spire_server: Option<String>,

    // --- Mesh ---
    /// Known peer iroh public keys (for LAN discovery bootstrap)
    pub known_peers: Vec<KnownPeer>,
    /// iroh relay server URLs (for cross-NAT connectivity)
    pub relay_urls: Vec<String>,

    // --- Zenoh ---
    /// Zenoh mode: router (CP) or client (MFT)
    pub zenoh_mode: String,
    /// Zenoh connect endpoints (for clients connecting to router)
    pub zenoh_connect: Vec<String>,

    // --- NORM ---
    /// NORM multicast group address
    pub norm_multicast_group: String,
    /// NORM FEC ratio (0.0-1.0)
    pub norm_fec_ratio: f32,
}

/// A known peer entry in the provisioning bundle.
#[derive(Debug, Serialize, Deserialize)]
pub struct KnownPeer {
    pub iroh_public_key: String,
    pub hostname: String,
    pub role: String,
    pub overlay_ip: Ipv4Addr,
}

pub fn generate(
    hostname: &str,
    role: &str,
    platform: &str,
    output_dir: &str,
    site: &str,
) -> Result<()> {
    let secret_key = SecretKey::generate(&mut rand::rng());
    let public_key = secret_key.public();
    let overlay_ip = derive_overlay_ip(&public_key);

    let zenoh_mode = match role {
        "cp" => "router",
        _ => "client",
    };

    let bundle = NodeBundle {
        version: 1,
        hostname: hostname.to_string(),
        role: role.to_string(),
        platform: platform.to_string(),
        site: site.to_string(),
        iroh_secret_key: secret_key.to_bytes().to_vec(),
        iroh_public_key: public_key.to_string(),
        overlay_ip,
        spire_join_token: None, // Generated when SPIRE server is available
        spire_trust_domain: format!("{site}.desertbread.net"),
        spire_server: None, // Set to CP's overlay IP when known
        known_peers: vec![],
        relay_urls: vec![
            "https://relay.desertbread.net".to_string(),
            "https://relay-isr.desertbread.net".to_string(),
            "https://relay-mum.desertbread.net".to_string(),
        ],
        zenoh_mode: zenoh_mode.to_string(),
        zenoh_connect: vec![], // Set to CP's overlay IP when known
        norm_multicast_group: "239.255.0.1:6003".to_string(),
        norm_fec_ratio: 0.2,
    };

    let output_path = Path::new(output_dir);
    std::fs::create_dir_all(output_path)?;

    let bundle_file = output_path.join(format!("{hostname}.json"));
    let json = serde_json::to_string_pretty(&bundle)?;
    std::fs::write(&bundle_file, &json)?;

    info!(
        hostname = %hostname,
        role = %role,
        site = %site,
        iroh_id = %public_key,
        overlay_ip = %overlay_ip,
        trust_domain = %bundle.spire_trust_domain,
        path = %bundle_file.display(),
        "bundle generated"
    );

    Ok(())
}

pub fn list(dir: &str) -> Result<()> {
    let path = Path::new(dir);
    if !path.exists() {
        println!("No bundles directory at {dir}");
        return Ok(());
    }

    println!("{:<20} {:<6} {:<10} {:<18} {:<44} TRUST DOMAIN", "HOSTNAME", "ROLE", "SITE", "OVERLAY IP", "IROH ID");
    println!("{}", "-".repeat(140));

    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|e| e == "json") {
            let contents = std::fs::read_to_string(entry.path())?;
            if let Ok(bundle) = serde_json::from_str::<NodeBundle>(&contents) {
                println!(
                    "{:<20} {:<6} {:<10} {:<18} {:<44} {}",
                    bundle.hostname,
                    bundle.role,
                    bundle.site,
                    bundle.overlay_ip,
                    bundle.iroh_public_key,
                    bundle.spire_trust_domain,
                );
            }
        }
    }

    Ok(())
}

fn derive_overlay_ip(id: &iroh::EndpointId) -> Ipv4Addr {
    let hash = Sha256::digest(id.as_bytes());
    let raw = u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]]);
    let mut host = raw & 0x003F_FFFF;
    let last_octet = host & 0xFF;
    if last_octet == 0 {
        host |= 1;
    } else if last_octet == 255 {
        host &= !1;
    }
    Ipv4Addr::from(0x6440_0000 | host)
}
