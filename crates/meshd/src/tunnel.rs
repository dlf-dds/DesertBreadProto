//! Tunnel driver trait — the abstraction boundary between mesh control and IP tunnel technology.
//!
//! # Why this exists
//!
//! The mesh daemon (`meshd`) has two jobs:
//!
//! 1. **Control plane** — peer discovery, identity exchange, handshake (handled by iroh QUIC)
//! 2. **Data plane** — IP-layer tunnel that carries actual user traffic between peers
//!
//! These concerns are intentionally decoupled. The control plane doesn't care whether
//! the tunnel is WireGuard, MASQUE, or something else — it just needs to tell the tunnel
//! "add this peer" or "remove this peer" after the handshake completes.
//!
//! `TunnelDriver` captures that boundary. The mesh protocol, discovery, and IPC code
//! all operate on `dyn TunnelDriver`, never on a concrete tunnel type.
//!
//! # Current implementation
//!
//! - **WireGuard** ([`crate::wireguard::WgInterface`]) — kernel WireGuard managed via
//!   `wg` and `ip` CLI tools. This is the production driver for Linux. On macOS (dev),
//!   it logs operations without actually creating interfaces.
//!
//! # Possible future implementations
//!
//! - **MASQUE over iroh** — HTTP/3 CONNECT-UDP proxying. Since iroh already gives us
//!   QUIC transport with NAT traversal, MASQUE could tunnel IP traffic without needing
//!   a kernel WireGuard module. Interesting for containers, restricted environments,
//!   or platforms where kernel modules aren't available.
//!
//! - **Userspace WireGuard** (e.g., boringtun) — same WireGuard protocol, no kernel
//!   module. Useful for unprivileged deployments.
//!
//! - **No-op / mock driver** — for testing and macOS development. Returns dummy keys,
//!   logs peer add/remove but doesn't create real interfaces. Cleaner than the current
//!   `cfg!(target_os)` guards scattered through `wireguard.rs`.
//!
//! # How to add a new driver
//!
//! 1. Create a new module (e.g., `masque.rs`) with a struct that holds driver state.
//! 2. Implement `TunnelDriver` for that struct. See [`crate::wireguard::WgInterface`]
//!    for a reference implementation.
//! 3. Add a CLI flag to `main.rs` to select the driver (e.g., `--tunnel=masque`).
//! 4. Construct the driver in `main.rs` and pass it as `Arc<dyn TunnelDriver>` to
//!    `MeshProtocol::new()`.
//!
//! The rest of the mesh stack (protocol handler, discovery loop, IPC server) will work
//! unchanged — they only interact with the tunnel through this trait.
//!
//! # Wire protocol note
//!
//! The mesh handshake ([`crate::peer::PeerHandshake`]) exchanges `tunnel_pubkey` and
//! `tunnel_endpoint` as opaque strings. Each driver interprets these in its own way:
//!
//! | Driver    | `tunnel_pubkey`                  | `tunnel_endpoint`      |
//! |-----------|----------------------------------|------------------------|
//! | WireGuard | Base64 Curve25519 public key     | `ip:port`              |
//! | MASQUE    | TBD (token, cert fingerprint...) | TBD (URI, ip:port...)  |
//!
//! The mesh protocol doesn't parse or validate these — it just ferries them between
//! peers and hands them to the tunnel driver.

use anyhow::Result;
use std::net::Ipv4Addr;

/// A tunnel driver that provides IP-layer connectivity between mesh peers.
///
/// Each mesh node has one `TunnelDriver` instance, created at startup and shared
/// (via `Arc<dyn TunnelDriver>`) across the protocol handler, discovery loop, and
/// IPC server. The driver must be `Send + Sync` since it's called from multiple
/// async tasks.
///
/// # Lifecycle
///
/// ```text
/// startup:   driver-specific constructor (e.g., WgInterface::setup())
///                 ↓
///            Arc<dyn TunnelDriver> passed to MeshProtocol::new()
///                 ↓
/// runtime:   add_peer() / remove_peer() / update_peer_endpoint()
///            called by protocol handler as peers come and go
///                 ↓
/// shutdown:  teardown() called from main()
/// ```
///
/// # What the driver is responsible for
///
/// - Managing its own network interface (creation happens in the constructor,
///   before the trait is used)
/// - Adding/removing peers with their tunnel credentials
/// - Providing its public key so the mesh handshake can exchange it with peers
///
/// # What the driver is NOT responsible for
///
/// - Peer discovery — that's iroh's job (mDNS, relay)
/// - Overlay IP assignment — that's [`crate::overlay_ip`], deterministic from iroh identity
/// - Identity or authentication — that's SPIRE's job (Phase 2)
pub trait TunnelDriver: Send + Sync + std::fmt::Debug {
    /// The public key or credential that remote peers need to authenticate this tunnel.
    ///
    /// For WireGuard: a base64-encoded Curve25519 public key.
    /// The mesh handshake sends this to every peer so they can configure their own
    /// tunnel to accept traffic from us.
    fn public_key(&self) -> &str;

    /// The overlay IP assigned to this node's tunnel interface.
    fn overlay_ip(&self) -> Ipv4Addr;

    /// Human-readable name of the tunnel interface (e.g., `"wg0"`, `"masque0"`).
    ///
    /// Used for logging and IPC status responses.
    fn interface_name(&self) -> &str;

    /// Register a new peer in the tunnel.
    ///
    /// Called by the protocol handler after a successful handshake with a new peer.
    ///
    /// - `pubkey`: the remote peer's tunnel public key (received in handshake)
    /// - `endpoint`: optional direct network address (`ip:port`) for the peer.
    ///   May be `None` if the peer is only reachable via relay.
    /// - `allowed_ip`: the peer's overlay IP — the tunnel should route traffic
    ///   for this IP through the tunnel to the peer.
    fn add_peer(
        &self,
        pubkey: &str,
        endpoint: Option<&str>,
        allowed_ip: Ipv4Addr,
    ) -> Result<()>;

    /// Update a peer's network endpoint without removing and re-adding.
    ///
    /// Called when a peer's address changes (e.g., it roams to a different network).
    fn update_peer_endpoint(&self, pubkey: &str, endpoint: &str) -> Result<()>;

    /// Remove a peer from the tunnel.
    ///
    /// Called when a peer is detected as offline (mDNS expiry, keepalive timeout).
    fn remove_peer(&self, pubkey: &str) -> Result<()>;

    /// Tear down the tunnel interface entirely.
    ///
    /// Called once during daemon shutdown. After this returns, the tunnel interface
    /// should no longer exist on the system.
    fn teardown(&self) -> Result<()>;
}
