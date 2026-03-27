# ADR 006: Deterministic Overlay IPs from iroh Public Key

## Status
Accepted

## Context
Nodes need IP addresses on the WireGuard overlay. Traditional approaches require a DHCP server or static allocation database — both are single points of failure.

## Decision
Derive overlay IPs deterministically from the iroh public key: SHA-256 hash truncated to 22 bits, mapped into the 100.64.0.0/10 CGNAT range.

## Rationale
- No allocation server. No DHCP. No state.
- Every node computes the same IP for the same key. Idempotent and convergent.
- 100.64.0.0/10 is the CGNAT range — will never conflict with real routable addresses.
- 22 bits = ~4M addresses. Collision probability is negligible for fleets under 10K nodes.
- If two keys do collide (birthday problem: ~2K nodes for 50% chance of any collision), the operational impact is detectable and resolvable by re-keying one node.

## Consequences
- IP addresses are not human-friendly. Use DNS or `fabric-cli peers` for lookup.
- Changing a node's iroh key changes its overlay IP. Keys should be generated once and persisted.
