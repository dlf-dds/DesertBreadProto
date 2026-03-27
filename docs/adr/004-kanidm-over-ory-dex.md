# ADR 004: Kanidm Over Ory/Dex

## Status
Accepted

## Context
Human operators need to authenticate to web-based tools (dashboards, COP displays). Need OAuth2/OIDC that works offline.

## Decision
Use Kanidm for human identity management.

## Rationale
- Single Rust binary. Ory is 4+ services (Hydra, Kratos, Keto, Oathkeeper). Dex is a federation proxy, not an identity store.
- Built-in replication between instances. Run primary in cloud, replicas per site. Syncs when connected, operates independently when not.
- Supports passkeys/WebAuthn, TOTP, backup codes natively.
- Can run fully offline.

## Consequences
- Kanidm is deferred until web UIs exist. If only SSH/CLI tools, SPIRE + SSH key distribution is sufficient.
- Kanidm replicas at each site add a small resource footprint to the CP node.
