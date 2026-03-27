# ADR 005: NORM for LAN Multicast

## Status
Accepted

## Context
Need to push large files (firmware, map tiles) to all nodes on a LAN simultaneously. Zenoh is pub/sub, not bulk multicast.

## Decision
Use NORM (NACK-Oriented Reliable Multicast, RFC 5740) for bulk LAN distribution. Zenoh handles structured data.

## Rationale
- NORM is purpose-built for reliable multicast with configurable FEC (Forward Error Correction).
- One sender, many receivers, UDP multicast, NACK-based loss recovery.
- LAN-only (multicast doesn't cross routers). For cross-site bulk transfer, use iroh-blobs.
- Different tool for a different job. Zenoh handles structured pub/sub data. NORM handles raw bulk distribution.

## Consequences
- NORM is a separate integration, not a Zenoh plugin. They coexist.
- FEC parameters must be tuned per-deployment: higher FEC for lossy WiFi, lower for clean Ethernet.
- NRL NORM library (C/C++) with Rust FFI bindings.
