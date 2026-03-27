# ADR 002: Raw WireGuard Bootstrapped by iroh

## Status
Accepted

## Context
iroh provides QUIC-based P2P connections but no IP-layer networking. Legacy tools (SSH, ATAK, any IP-based application) need a virtual IP interface.

## Decision
Use raw WireGuard (kernel module on Linux) for the IP overlay, bootstrapped and managed by iroh via the `meshd` daemon.

## Rationale
- WireGuard is a kernel module that does one thing perfectly: encrypted point-to-point tunnels.
- iroh handles the hard part: peer discovery, NAT traversal, key exchange signaling.
- `meshd` is ~500 lines of glue: it runs an iroh endpoint, manages a WireGuard interface, and dynamically adds/removes peers based on iroh events.
- Overlay IPs are derived deterministically from iroh public keys (SHA-256 → 100.64.0.0/10). No DHCP, no allocation server, no state.

## Consequences
- `meshd` is the only custom component in the connection layer.
- WireGuard peer configuration is fully dynamic — no static peer entries.
- Requires `wg` and `ip` CLI tools on Linux. macOS dev mode uses no-ops.
