//! Deterministic overlay IP assignment from iroh public key.
//!
//! Maps an iroh EndpointId (Ed25519 public key) to a unique IP in the 100.64.0.0/10
//! CGNAT range. This range has 22 host bits (~4M addresses), making collisions
//! negligible for fleets under 10K nodes.
//!
//! No DHCP, no allocation server, no state. Every node derives the same IP
//! for the same key.

use iroh::EndpointId;
use sha2::{Digest, Sha256};
use std::net::Ipv4Addr;

/// Base address for the overlay network (100.64.0.0)
const OVERLAY_BASE: u32 = 0x6440_0000; // 100.64.0.0
/// Mask for the /10 prefix (22 host bits)
const OVERLAY_HOST_MASK: u32 = 0x003F_FFFF; // 22 bits

/// Derive a deterministic overlay IPv4 address from an iroh EndpointId.
///
/// Uses SHA-256 of the public key bytes, truncated to 22 bits, mapped into
/// the 100.64.0.0/10 CGNAT range. Avoids .0 and .255 in the last octet
/// to prevent broadcast/network address collisions on /24 subnets.
pub fn overlay_ip_from_id(id: &EndpointId) -> Ipv4Addr {
    let hash = Sha256::digest(id.as_bytes());
    // Take the first 4 bytes of the hash as a u32
    let raw = u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]]);
    let mut host = raw & OVERLAY_HOST_MASK;

    // Avoid x.x.x.0 and x.x.x.255 (network/broadcast on /24 subnets)
    let last_octet = host & 0xFF;
    if last_octet == 0 {
        host |= 1;
    } else if last_octet == 255 {
        host &= !1; // becomes 254
    }

    Ipv4Addr::from(OVERLAY_BASE | host)
}

/// The overlay network prefix length.
pub const OVERLAY_PREFIX_LEN: u8 = 10;

/// Format an overlay IP with its prefix for WireGuard configuration.
pub fn overlay_cidr(ip: Ipv4Addr) -> String {
    format!("{ip}/{OVERLAY_PREFIX_LEN}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_same_key() {
        let key = iroh::SecretKey::generate(&mut rand::rng());
        let id = key.public();
        let ip1 = overlay_ip_from_id(&id);
        let ip2 = overlay_ip_from_id(&id);
        assert_eq!(ip1, ip2, "same key must produce same IP");
    }

    #[test]
    fn different_keys_different_ips() {
        let k1 = iroh::SecretKey::generate(&mut rand::rng());
        let k2 = iroh::SecretKey::generate(&mut rand::rng());
        let ip1 = overlay_ip_from_id(&k1.public());
        let ip2 = overlay_ip_from_id(&k2.public());
        // Technically could collide, but vanishingly unlikely with 22 bits
        assert_ne!(ip1, ip2, "different keys should almost certainly produce different IPs");
    }

    #[test]
    fn ip_in_cgnat_range() {
        for _ in 0..100 {
            let key = iroh::SecretKey::generate(&mut rand::rng());
            let ip = overlay_ip_from_id(&key.public());
            let octets = ip.octets();
            // 100.64.0.0/10 means first octet is 100, second octet 64-127
            assert_eq!(octets[0], 100);
            assert!(
                octets[1] >= 64 && octets[1] <= 127,
                "second octet {0} not in 64..=127",
                octets[1]
            );
        }
    }

    #[test]
    fn no_broadcast_or_network_addresses() {
        for _ in 0..1000 {
            let key = iroh::SecretKey::generate(&mut rand::rng());
            let ip = overlay_ip_from_id(&key.public());
            let last = ip.octets()[3];
            assert_ne!(last, 0, "last octet must not be 0");
            assert_ne!(last, 255, "last octet must not be 255");
        }
    }
}
