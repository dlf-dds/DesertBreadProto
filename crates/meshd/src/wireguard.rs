//! WireGuard tunnel driver — the default [`TunnelDriver`] implementation.
//!
//! Manages a kernel WireGuard interface via `wg` and `ip` CLI tools (not netlink),
//! keeping operations simple, auditable, and easy to debug in the field.
//!
//! On Linux, this creates real WireGuard interfaces. On macOS (dev mode), all
//! interface operations are no-ops that log what they would do — this lets you
//! run `meshd` locally for development without root or a WireGuard kernel module.
//!
//! # Relationship to [`TunnelDriver`]
//!
//! This is the production implementation of [`crate::tunnel::TunnelDriver`].
//! The mesh protocol never calls `WgInterface` methods directly — it goes through
//! the trait. If you're adding a new tunnel technology, use this as your reference
//! implementation and see [`crate::tunnel`] for the full design rationale.

use crate::overlay_ip;
use crate::tunnel::TunnelDriver;
use std::net::Ipv4Addr;
use std::process::Command;
use thiserror::Error;
use tracing::{debug, info, warn};

#[derive(Error, Debug)]
pub enum WgError {
    #[error("command failed: {cmd}: {stderr}")]
    CommandFailed { cmd: String, stderr: String },
    #[error("wg tool not found — is wireguard-tools installed?")]
    WgNotFound,
    #[error("ip tool not found")]
    IpNotFound,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// WireGuard keypair (base64 encoded strings as wg produces them).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WgKeypair {
    pub private_key: String,
    pub public_key: String,
}

/// State for a managed WireGuard interface.
#[derive(Debug)]
pub struct WgInterface {
    pub name: String,
    pub listen_port: u16,
    pub keypair: WgKeypair,
    pub overlay_ip: Ipv4Addr,
}

/// Generate a WireGuard keypair using the `wg` tool.
pub fn generate_keypair() -> Result<WgKeypair, WgError> {
    let privkey_out = Command::new("wg").arg("genkey").output()?;
    if !privkey_out.status.success() {
        return Err(WgError::WgNotFound);
    }
    let private_key = String::from_utf8_lossy(&privkey_out.stdout).trim().to_string();

    let pubkey_out = Command::new("wg")
        .arg("pubkey")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(private_key.as_bytes())?;
            child.wait_with_output()
        })?;

    let public_key = String::from_utf8_lossy(&pubkey_out.stdout).trim().to_string();

    Ok(WgKeypair {
        private_key,
        public_key,
    })
}

impl WgInterface {
    /// Create and configure the WireGuard interface.
    ///
    /// This is the driver-specific constructor — not part of `TunnelDriver` because
    /// each driver has its own setup parameters. After calling this, wrap the result
    /// in `Arc<dyn TunnelDriver>` and pass it to `MeshProtocol::new()`.
    ///
    /// On macOS (dev), this is a no-op that logs what it would do.
    /// On Linux, it creates the wg0 interface, assigns the overlay IP, and brings it up.
    pub fn setup(name: &str, listen_port: u16, overlay_ip: Ipv4Addr) -> Result<Self, WgError> {
        let keypair = generate_keypair()?;
        let cidr = overlay_ip::overlay_cidr(overlay_ip);

        if cfg!(target_os = "linux") {
            // Create interface
            run_cmd("ip", &["link", "add", name, "type", "wireguard"])?;

            // Set private key via temp file (wg setconf needs a file or stdin)
            let tmpfile = format!("/tmp/.wg-{name}-privkey");
            std::fs::write(&tmpfile, &keypair.private_key)?;
            run_cmd(
                "wg",
                &[
                    "set",
                    name,
                    "listen-port",
                    &listen_port.to_string(),
                    "private-key",
                    &tmpfile,
                ],
            )?;
            std::fs::remove_file(&tmpfile).ok();

            // Assign IP and bring up
            run_cmd("ip", &["addr", "add", &cidr, "dev", name])?;
            run_cmd("ip", &["link", "set", name, "up"])?;

            info!(interface = name, ip = %overlay_ip, port = listen_port, "WireGuard interface up");
        } else {
            info!(
                interface = name,
                ip = %overlay_ip,
                port = listen_port,
                "WireGuard setup skipped (non-Linux dev mode)"
            );
        }

        Ok(Self {
            name: name.to_string(),
            listen_port,
            keypair,
            overlay_ip,
        })
    }
}

impl TunnelDriver for WgInterface {
    fn public_key(&self) -> &str {
        &self.keypair.public_key
    }

    fn overlay_ip(&self) -> Ipv4Addr {
        self.overlay_ip
    }

    fn interface_name(&self) -> &str {
        &self.name
    }

    fn add_peer(
        &self,
        pubkey: &str,
        endpoint: Option<&str>,
        allowed_ip: Ipv4Addr,
    ) -> anyhow::Result<()> {
        let allowed = format!("{allowed_ip}/32");

        if cfg!(target_os = "linux") {
            let mut args = vec!["set", &self.name, "peer", pubkey, "allowed-ips", &allowed];
            if let Some(ep) = endpoint {
                args.extend(["endpoint", ep]);
            }
            // 25 second keepalive to maintain NAT mappings
            args.extend(["persistent-keepalive", "25"]);
            run_cmd("wg", &args)?;
            info!(peer = pubkey, ip = %allowed_ip, "WireGuard peer added");
        } else {
            debug!(peer = pubkey, ip = %allowed_ip, "WireGuard peer add (dev no-op)");
        }
        Ok(())
    }

    fn update_peer_endpoint(&self, pubkey: &str, endpoint: &str) -> anyhow::Result<()> {
        if cfg!(target_os = "linux") {
            run_cmd(
                "wg",
                &["set", &self.name, "peer", pubkey, "endpoint", endpoint],
            )?;
            debug!(peer = pubkey, endpoint, "WireGuard peer endpoint updated");
        } else {
            debug!(peer = pubkey, endpoint, "WireGuard peer endpoint update (dev no-op)");
        }
        Ok(())
    }

    fn remove_peer(&self, pubkey: &str) -> anyhow::Result<()> {
        if cfg!(target_os = "linux") {
            run_cmd("wg", &["set", &self.name, "peer", pubkey, "remove"])?;
            info!(peer = pubkey, "WireGuard peer removed");
        } else {
            debug!(peer = pubkey, "WireGuard peer remove (dev no-op)");
        }
        Ok(())
    }

    fn teardown(&self) -> anyhow::Result<()> {
        if cfg!(target_os = "linux") {
            run_cmd("ip", &["link", "del", &self.name])?;
            info!(interface = %self.name, "WireGuard interface removed");
        }
        Ok(())
    }
}

fn run_cmd(program: &str, args: &[&str]) -> Result<(), WgError> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let cmd = format!("{program} {}", args.join(" "));
        warn!(%cmd, %stderr, "command failed");
        return Err(WgError::CommandFailed { cmd, stderr });
    }
    Ok(())
}
