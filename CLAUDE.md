# Desert Bread Proto — Tactical Data Fabric

## What This Is

A self-healing, identity-based mesh network for tactical edge operations. Runs entirely from edge hardware with no cloud dependency. Cloud infrastructure (when reachable) adds efficiency — relay for NAT traversal, federated identity, cross-site data aggregation.

**This is NOT the previous Netbird-based DesertBreadBird.** This is a clean-break rebuild using iroh + raw WireGuard.

## Architecture (Four Planes)

1. **Connection Plane (iroh)** — QUIC-based P2P connections, mDNS local discovery, NAT traversal via relay
2. **IP Overlay Plane (WireGuard)** — Raw kernel WireGuard bootstrapped by iroh via `meshd` daemon
3. **Data Plane (Zenoh + NORM)** — Pub/sub data fabric with anti-entropy reconciliation + reliable LAN multicast
4. **Identity Plane (SPIRE/SPIFFE + Kanidm)** — Per-site sovereign CA with federation, human identity via Kanidm

## Key Components

| Crate | Purpose |
| --- | --- |
| `meshd` | iroh–tunnel bridge daemon. Tunnel tech is pluggable via `TunnelDriver` trait (`tunnel.rs`); WireGuard is the current driver (`wireguard.rs`) |
| `provision` | Node provisioning tool (generates bundles, flashes config) |
| `fabric-cli` | Operator CLI for mesh status, debugging, manual overrides |

## Development

```bash
# Build all crates
cargo build

# Run meshd (dev mode — WireGuard ops are no-ops on macOS)
cargo run --bin meshd -- --key-file /tmp/meshd-dev.key

# Run tests
cargo test

# Cross-compile for aarch64 (RPi, AGX Orin)
cargo build --target aarch64-unknown-linux-gnu

# Terraform (relay infrastructure)
cd terraform && aws-vault exec cochlearis -- terraform plan
```

## Environment

- AWS account: `cochlearis` (eu-central-1). Always use `aws-vault exec cochlearis --no-session`.
- Cloudflare domain: `desertbread.net`. DNS records NOT proxied (breaks QUIC/gRPC).
- Secrets in `.envrc.local` (gitignored). Never commit secrets.
- Dev machine is macOS (aarch64-apple-darwin). Deployment targets are Linux (Ubuntu 24.04).

## Conventions

- Rust edition 2024, MSRV 1.85
- `anyhow` for application errors, `thiserror` for library errors
- `tracing` for all logging (not `log` or `println!`)
- `postcard` for compact binary wire formats between mesh peers
- `serde_json` for human-readable config and provisioning bundles
- `clap` derive API for all CLIs
- No `unwrap()` in library code. `expect()` only with descriptive messages.
- Integration tests in `tests/integration/`. Unit tests in-module.

## Constraints

- Bahrain (me-south-1) and UAE (me-central-1) AWS regions are destroyed. Never deploy there.
- Star Shield has no port forwarding. All connections must be outbound-initiated.
- All deployment targets are Linux. macOS is dev-only.
- FIPS 140-3 and PQC are Phase 4. Don't add them early.

## ADR Decisions (Do Not Revisit)

1. iroh over Nebula/Netbird/Headscale
2. Raw WireGuard bootstrapped by iroh
3. SPIRE/SPIFFE per-site with federation
4. Kanidm over Ory/Dex
5. NORM for LAN multicast, not Zenoh
6. Deterministic overlay IPs from iroh public key
