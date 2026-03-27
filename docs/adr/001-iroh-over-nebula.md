# ADR 001: iroh Over Nebula/Netbird/Headscale

## Status
Accepted

## Context
We need a connection layer that provides peer discovery, NAT traversal, and encrypted transport. Candidates: Nebula, Netbird, Headscale, iroh.

## Decision
Use iroh as the connection layer.

## Rationale
- **No central control plane.** iroh peers discover each other via mDNS (LAN), DNS, or relay. No single server whose failure breaks the mesh.
- **QUIC-native NAT traversal.** ~90% direct connections via holepunching, relay fallback for the rest.
- **mDNS local discovery.** Peers on the same LAN find each other with zero configuration, zero internet.
- **QUIC multipath.** Connections survive interface changes (WiFi → LTE failover).
- **Content-addressed data sync** (iroh-blobs) and **gossip** (iroh-gossip) for higher-layer protocols.

Nebula's development is stagnant and mobile clients are crippled. Netbird and Headscale are single-server architectures with no federation story — exactly the single point of failure we're eliminating.

## Consequences
- iroh does NOT provide IP-layer networking (no TUN device). We bridge this gap with raw WireGuard via `meshd`.
- We depend on the iroh crate ecosystem, which is actively developed but still pre-1.0.
- Self-hosted relay servers must be deployed for cross-NAT connectivity.
