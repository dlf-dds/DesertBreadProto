# ADR 003: SPIRE/SPIFFE Per-Site with Federation

## Status
Accepted

## Context
Every connection must be mutually authenticated. We need workload identity that works when disconnected from the internet.

## Decision
Run a SPIRE server per site, each sovereign over its own trust domain. Federation via trust bundle exchange when connectivity exists.

## Rationale
- Each site's SPIRE server is an independent CA. It issues certs, attests workloads, and rotates keys with no upstream dependency.
- Federation = "I trust certs from your domain." Not "we share a database." No split-brain possible.
- Trust domains: `spiffe://alpha.desertbread.net`, `spiffe://bravo.desertbread.net`, `spiffe://cloud.desertbread.net`.
- When partitioned, each site continues issuing certs locally. When reconnected, trust bundles sync. No reconciliation needed.

## Consequences
- Root CA ceremony is required (air-gapped, offline).
- Each SPIRE server holds an intermediate CA cert signed by the root.
- Short-lived certs (1-24 hours) mean compromise of a single cert has limited blast radius.
