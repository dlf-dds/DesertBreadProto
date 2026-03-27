//! Node provisioning bundle generation.
//!
//! Generates everything a node needs to join the mesh:
//! - iroh keypair
//! - WireGuard keypair
//! - SPIRE join token (placeholder for Phase 2)
//! - Mesh configuration (known peers, relay info, site config)

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
    pub hostname: String,
    pub role: String,
    pub platform: String,
    pub site: String,
    pub iroh_secret_key: Vec<u8>,
    pub iroh_public_key: String,
    pub overlay_ip: Ipv4Addr,
    pub known_peers: Vec<String>,
    pub relay_urls: Vec<String>,
}

pub fn generate(
    hostname: &str,
    role: &str,
    platform: &str,
    output_dir: &str,
    site: &str,
) -> Result<()> {
    // Generate iroh keypair
    let secret_key = SecretKey::generate(&mut rand::rng());
    let public_key = secret_key.public();

    // Derive overlay IP
    let overlay_ip = crate_overlay_ip(&public_key);

    let bundle = NodeBundle {
        hostname: hostname.to_string(),
        role: role.to_string(),
        platform: platform.to_string(),
        site: site.to_string(),
        iroh_secret_key: secret_key.to_bytes().to_vec(),
        iroh_public_key: public_key.to_string(),
        overlay_ip,
        known_peers: vec![],
        relay_urls: vec![],
    };

    // Write bundle to output directory
    let output_path = Path::new(output_dir);
    std::fs::create_dir_all(output_path)?;

    let bundle_file = output_path.join(format!("{hostname}.json"));
    let json = serde_json::to_string_pretty(&bundle)?;
    std::fs::write(&bundle_file, &json)?;

    info!(
        hostname = %hostname,
        iroh_id = %public_key,
        overlay_ip = %overlay_ip,
        path = %bundle_file.display(),
        "bundle generated"
    );

    Ok(())
}

pub fn list(dir: &str) -> Result<()> {
    let path = Path::new(dir);
    if !path.exists() {
        info!("no bundles directory found at {dir}");
        return Ok(());
    }

    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|e| e == "json") {
            let contents = std::fs::read_to_string(entry.path())?;
            if let Ok(bundle) = serde_json::from_str::<NodeBundle>(&contents) {
                println!(
                    "{:<20} {:<6} {:<10} {:<16} {}",
                    bundle.hostname, bundle.role, bundle.site, bundle.overlay_ip, bundle.iroh_public_key
                );
            }
        }
    }

    Ok(())
}

/// Derive overlay IP from iroh public key (duplicates meshd logic for standalone use).
fn crate_overlay_ip(id: &iroh::EndpointId) -> Ipv4Addr {
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
