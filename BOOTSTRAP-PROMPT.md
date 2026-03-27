# Tactical Data Fabric — Bootstrap Prompt

You are building a software-defined tactical data fabric from scratch. This is the complete specification. Read it entirely before writing any code.

## What This System Is

A self-healing, identity-based mesh network that runs entirely from the edge. The complete capability — discovery, identity, data fabric, administration — can be stood up on a LAN with no internet, no cloud, no external dependencies. Cloud infrastructure, when reachable, adds efficiency: geographic relay for cross-site NAT traversal, federated identity across trust domains, cross-site data aggregation, and operational monitoring. But cloud is an accelerant, not a foundation. Remove it and the system still works. Start without it and the system still stands up.

This is not a VPN product. It is a tactical communications fabric for DDIL (Denied, Degraded, Intermittent, Limited) environments. Nodes are sensors, effectors, command posts, and data services deployed across physical sites connected by unreliable links. The system must work when the internet is gone, when sites are partitioned from each other, and when they reconnect hours or days later. It must also work when the internet was never there to begin with.

## Design Principles

- No single point of failure. No component whose loss breaks the mesh.
- No split-brain reconciliation problems. Each site is sovereign when disconnected. Federation reconciles naturally.
- Edge-first. The entire system stands up from local hardware with no cloud, no internet, no external services. Cloud is an accelerant — it improves efficiency and reach when available, but is never required. A site that has never seen the internet is fully functional.
- Pre-provisioned. All devices are enrolled and configured before deployment. Field enrollment is an emergency capability, not the primary path.
- Minimal custom code. Integrate proven open-source components. The only custom code is the thin glue between them.
- Every component is open-source, auditable, and forkable. No vendor lock-in. No proprietary protocols.
- Idempotent. Every script, every configuration, every deployment step can be re-run safely.

## Data Sovereignty

Commercial cloud infrastructure (AWS, Azure, GCP, Cloudflare, etc.) can be used freely for services that only facilitate coordination — they relay encrypted traffic, broker connections, or distribute configuration. They never see plaintext mission data, never store privileged content, and never make trust decisions. These are orchestration services and can run wherever makes operational sense (latency, cost, availability).

Services that contain or process privileged data must run only in sovereign-controlled locations:

| Data sensitivity | Permitted locations |
|---|---|
| **Unclassified / CUI** | Operator physical custody (edge hardware forward), CONUS-controlled facility, IL4/IL5 cloud enclaves |
| **Classified** | IL6 cloud enclaves, DISA JOE sites (e.g., AWS Outpost Germany), SCIFs, operator physical custody in authorized facilities |

**How this maps to the architecture:**

| Component | Contains privileged data? | Where it runs |
|---|---|---|
| **iroh relay** | No. Relays encrypted QUIC streams. Cannot decrypt. | Commercial cloud, anywhere sensible |
| **Cloudflare DNS** | No. Public DNS records only. | Cloudflare edge |
| **SPIRE Server (cloud)** | Borderline. Holds signing keys for cloud trust domain. Does not hold site CA keys. | IL4/IL5 if issuing certs for CUI workloads. Commercial cloud if limited to federation bundle exchange. |
| **Kanidm (cloud primary)** | Yes. Operator identities, group memberships, authentication credentials. | IL4/IL5 minimum. IL6 if operators handle classified. |
| **Zenoh cloud router** | Yes, if aggregating mission data. No, if only routing. | Sovereign-controlled if storing data. Commercial cloud if pass-through only. |
| **WireGuard overlay traffic** | The tunnel payload is mission data. The tunnel metadata (endpoints, handshakes) is not. | Tunnel terminates on sovereign hardware (edge nodes). Relay of encrypted WireGuard UDP is just facilitation — the relay never sees plaintext. |
| **SPIRE Server (site)** | Yes. Site CA signing key. Workload attestation records. | Edge hardware in operator physical custody. |
| **Kanidm (site replica)** | Yes. Replicated operator identities. | Edge hardware in operator physical custody. |
| **Zenoh storage (site)** | Yes. Mission data. | Edge hardware in operator physical custody. |

**The principle:** if a service can be replaced with a dumb pipe and nothing breaks, it is facilitation and can run on commercial cloud. If removing the service loses data or trust authority, it is sovereign and must be in a controlled location. When in doubt, ask: "if this VM were seized by a foreign adversary, what do they get?" If the answer is "encrypted blobs they can't decrypt," it's facilitation. If the answer is "signing keys, operator identities, or mission data," it's sovereign.

## Architecture: Four Planes

### 1. Connection Plane (iroh)

iroh is the foundation. It handles peer discovery, NAT traversal, and encrypted transport. Every node runs iroh.

**What iroh provides:**
- QUIC-based end-to-end encrypted P2P connections, addressed by public key (not IP)
- NAT traversal via QUIC holepunching (~90% direct connections, relay fallback)
- Self-hosted relay servers (open-source binary) for when direct P2P fails
- mDNS local discovery — peers on the same LAN find each other without internet, without any server
- QUIC multipath — connections survive network interface changes (WiFi to LTE failover)
- Gossip protocol (iroh-gossip) for pub/sub overlay between peers
- Content-addressed data sync (iroh-blobs) for verified bulk transfer

**What iroh does NOT provide:**
- IP-layer networking (no TUN device, no virtual interface, no `ip route`)
- A VPN. You cannot SSH over iroh. Legacy IP-based tools cannot use iroh directly.

**Cloud relays:** Deploy iroh relay servers in multiple cloud regions (AWS eu-central-1, ap-south-1, il-central-1 — avoid me-south-1 Bahrain and me-central-1 UAE). These are stateless, horizontally scalable, and disposable. Run at least 3 in geographically diverse regions. Each site also runs a local relay for LAN-only operation.

**Identity in iroh:** Each node has a keypair. The public key IS the node's identity. iroh connections are authenticated by proving possession of the private key. This is the transport-layer identity — SPIRE provides the higher-layer workload identity.

### 2. IP Overlay Plane (WireGuard)

Raw WireGuard (kernel module on Linux, wireguard-go on other platforms) provides the IP-layer overlay. This is for SSH, legacy tools, ATAK, anything that expects to connect to an IP address and port.

**WireGuard is NOT the mesh.** It is a utility layer bootstrapped by iroh. The mesh is iroh. WireGuard provides IP compatibility.

**The bridge daemon (custom component, ~500 lines of Rust):**

Every node runs a small daemon called `meshd` that:
1. Runs an iroh endpoint for peer discovery and signaling
2. Manages a local WireGuard interface (`wg0`)
3. Generates a WireGuard keypair at first boot, publishes the public key via iroh
4. Listens for peer discovery events from iroh (new peer found, peer endpoint changed, peer lost)
5. On new peer: receives peer's WireGuard public key over iroh's authenticated QUIC channel, adds a WireGuard peer entry (`wg set wg0 peer <pubkey> endpoint <ip:port> allowed-ips <overlay-ip>/32`)
6. On peer endpoint change: updates the WireGuard peer's endpoint
7. On peer lost: removes the WireGuard peer entry
8. Assigns overlay IPs from a deterministic scheme based on the node's iroh public key (hash to 100.64.0.0/10 space)
9. Publishes its overlay IP via iroh so peers can set `allowed-ips` correctly

**IP allocation:** Overlay IPs are derived deterministically from the iroh public key. `100.64.0.0/10` is the CGNAT range — used here because it will never conflict with real routable addresses. The derivation must be collision-resistant: truncate SHA-256 of the iroh public key to fit the /10 space (22 bits = ~4M addresses). Every node computes the same IP for the same key — no DHCP, no allocation server, no state.

**WireGuard configuration per node:**
```ini
[Interface]
PrivateKey = <generated at first boot>
Address = <deterministic from iroh pubkey>/10
ListenPort = 51820
```

Peer entries are added/removed dynamically by `meshd`. No static configuration.

### 3. Data Plane (Zenoh + NORM)

#### Zenoh

Zenoh is the application-layer data fabric. All mission data flows through Zenoh: sensor readings, tracks, commands, COP updates, alerts.

**What Zenoh provides:**
- Pub/sub with configurable reliability (BEST_EFFORT for telemetry, RELIABLE for commands)
- Distributed queryable storage — each node can store data locally and serve it to peers
- Anti-entropy reconciliation — when partitioned sites reconnect, Zenoh storages align via Merkle-tree delta exchange. No full resync, no manual intervention.
- Late joiner catch-up — TRANSIENT_LOCAL durability means new/reconnecting subscribers get historical data
- Configurable QoS per topic (priority, congestion control, express flag)
- zenoh-pico for microcontrollers (15KB flash, 12KB RAM) — same protocol, same namespace, different runtime
- DDS bridge (zenoh-plugin-dds) for NATO STANAG 4586 and ROS 2 interoperability

**Transport configuration:** Configure Zenoh to use iroh as its underlying transport where possible. Zenoh supports pluggable transports (TCP, UDP, QUIC, TLS, serial). For cross-site links that traverse NAT, Zenoh traffic should flow over the iroh QUIC connections (which handle NAT traversal). For LAN-local traffic, Zenoh can use multicast UDP discovery and direct TCP/UDP.

**Topic namespace design:**
```
{site}/{node-role}/{data-type}/{instance}

Examples:
alpha/sensor/eo-ir/01          # EO/IR sensor feed from site alpha, sensor 01
alpha/c2/tracks/fused          # Fused track picture at site alpha
alpha/effector/fires/status    # Fires system status
global/identity/certs          # Certificate distribution (all sites)
global/policy/acl              # Access control policy updates
```

**Storage placement:**
- Every Command Post (AGX Orin) node runs a Zenoh storage for its site's data
- Cloud VM runs a Zenoh storage for cross-site aggregation (when reachable)
- MFT (RPi) nodes are lightweight pub/sub endpoints — they do not run storage

**Island reconciliation:** When two sites that were partitioned reconnect (e.g., internet comes back, or a relay node bridges them), their Zenoh storages automatically align. This is Zenoh's built-in anti-entropy — Merkle trees identify the delta, only changed/missing data is exchanged. No application code needed.

#### NORM (RFC 5740)

NACK-Oriented Reliable Multicast for bulk data distribution within a site's LAN.

**When to use NORM vs Zenoh:**
- Zenoh: structured pub/sub, queries, storage alignment. Application semantics.
- NORM: raw bulk multicast. Pushing a large file, firmware image, or map tileset to all nodes on a LAN simultaneously. One sender, many receivers, UDP multicast, NACK-based loss recovery, configurable FEC (Forward Error Correction).

**Implementation:**
- Use the NRL NORM library (C/C++, BSD license). Rust bindings via FFI or the `norm` crate if available.
- NORM operates on the LAN only (multicast does not cross routers without explicit configuration). For cross-site bulk transfer, use iroh-blobs (content-addressed, verified, resumable).
- FEC parameters are tunable per transfer: higher FEC ratio for lossy links (WiFi, congested LAN), lower for clean links.

**Integration with Zenoh:** NORM is not a Zenoh transport — it is a separate tool for a specific job (bulk multicast on LAN). Zenoh handles structured data and reconciliation. NORM handles bulk distribution. They coexist, they don't integrate.

### 4. Identity and Security Plane (SPIRE/SPIFFE + Kanidm + X.509/PQC)

#### SPIRE/SPIFFE — Workload Identity

SPIRE is the identity foundation. Every workload (process, container, daemon) on every node gets a cryptographically attested SPIFFE identity (an X.509-SVID or JWT-SVID).

**Architecture:**
- Each site runs a SPIRE Server. The SPIRE Server is the certificate authority for that site's trust domain.
- Each node runs a SPIRE Agent. The agent attests the node's identity (hardware, OS, process attributes) and requests SVIDs from the SPIRE Server.
- Trust domain per site: `spiffe://alpha.desertbread.net`, `spiffe://bravo.desertbread.net`, etc.
- Cloud runs its own SPIRE Server: `spiffe://cloud.desertbread.net`

**Federation:**
- SPIRE servers federate by exchanging trust bundles (public keys). This is built-in to SPIRE.
- When a site has internet, its SPIRE server exchanges bundles with the cloud SPIRE server and with other sites' SPIRE servers.
- When a site is disconnected, its SPIRE server continues issuing certs locally. It is sovereign over its own trust domain.
- When connectivity returns, trust bundles sync. No reconciliation needed — each trust domain is independent. Federation means "I trust certs from your domain" not "we share a database."
- This is the key architectural property: federation without split-brain. Each SPIRE server is authoritative for its own domain. There is no shared state to diverge.

**What SPIRE provides:**
- Automated X.509 certificate issuance and rotation (short-lived certs, auto-renewed)
- Workload attestation (prove that this process is what it claims to be)
- Node attestation (prove that this node is hardware we trust)
- mTLS everywhere — every connection between workloads is mutually authenticated
- No static secrets (no passwords, no API keys, no long-lived tokens stored on disk)

**Integration with iroh:** iroh authenticates at the transport layer via its own keypair. SPIRE authenticates at the workload layer via X.509 SVIDs. Both layers of identity must be present: iroh proves "this is the node I think it is" and SPIRE proves "this workload on that node is authorized to do what it's asking."

**Integration with WireGuard:** The `meshd` daemon validates that a peer's iroh public key corresponds to a node with a valid SPIRE-issued SVID before adding it as a WireGuard peer. This prevents a stolen iroh keypair from joining the WireGuard overlay without also having valid workload attestation.

**Integration with Zenoh:** Zenoh connections use mTLS with SPIRE-issued certificates. Zenoh's TLS transport is configured with SPIRE-provided cert/key material (via the Workload API or SPIFFE Helper sidecar).

#### Kanidm — Human Identity

Kanidm handles human operator authentication: who is this person, and what groups do they belong to? This is separate from workload identity.

**Why Kanidm and not Ory or Dex:**
- Kanidm is a single Rust binary. Ory is 4+ services (Hydra, Kratos, Keto, Oathkeeper). Dex is a federation proxy, not an identity store.
- Kanidm is designed for security-first environments. It supports passkeys/WebAuthn, TOTP, and backup codes natively.
- Kanidm can run offline. Ory and Dex both assume always-on cloud connectivity.
- Kanidm supports replication between instances (built-in, not bolt-on). Run one in the cloud, one per site, they sync when connected.

**Architecture:**
- Cloud: Kanidm primary instance. Operators manage users/groups here when connected.
- Per site: Kanidm read replica. Syncs from cloud when connected. Serves local authentication when disconnected.
- Authentication flow: Operator authenticates to local Kanidm instance → receives OAuth2/OIDC token → token is validated by local services (dashboards, admin panels, COP displays).

**When Kanidm is NOT needed:** If there are no web UIs, no dashboards, no browser-based tools — only SSH and CLI — then SPIRE + SSH key distribution is sufficient. Kanidm is for human-facing authentication to web services. Include it only if you have web-based operator interfaces.

**Do not roll your own IdP.** The OAuth2/OIDC protocol surface area is enormous and subtle. Kanidm or similar battle-tested implementation handles token issuance, refresh, revocation, PKCE, session management, and credential storage correctly. Building this from SPIRE primitives would be reimplementing an IdP poorly.

#### X.509 Certificate Chain and Post-Quantum Cryptography

**Certificate hierarchy:**
```
Root CA (offline, air-gapped)
├── Cloud Intermediate CA (SPIRE Server @ cloud.desertbread.net)
├── Site Alpha Intermediate CA (SPIRE Server @ alpha.desertbread.net)
├── Site Bravo Intermediate CA (SPIRE Server @ bravo.desertbread.net)
└── ...
```

- Root CA key is generated on an air-gapped machine. Used only to sign intermediate CA certs for SPIRE servers. Stored offline (hardware security module if available, otherwise encrypted USB in a safe).
- Each SPIRE server holds an intermediate CA cert signed by the root. It issues short-lived (1-24 hour) workload certs.
- SPIRE handles all cert rotation automatically. No manual cert management after initial setup.

**Post-Quantum Cryptography (Phase 2+):**
- ML-KEM (CRYSTALS-Kyber) for key encapsulation. Used in the X.509 cert chain for key agreement.
- ML-DSA (CRYSTALS-Dilithium) for digital signatures. Used for cert signing.
- Rosenpass for WireGuard PQC key exchange (adds a PQ handshake on top of WireGuard's Noise protocol).
- Implementation: use AWS-LC (FIPS 140-3 validated, includes ML-KEM and ML-DSA) or OpenSSL 3.x with oqs-provider (Open Quantum Safe).
- PQC is additive — it augments existing crypto, does not replace it. Hybrid mode: classical + PQ algorithms run in parallel. If PQ is broken, classical still protects.

**FIPS 140-3:**
- WireGuard kernel module uses ChaCha20-Poly1305 (not FIPS-validated in the kernel). For FIPS compliance, use wireguard-go with OpenSSL FIPS module or AWS-LC.
- All TLS connections (Zenoh, SPIRE, Kanidm) use OpenSSL or AWS-LC in FIPS mode.
- FIPS validation is per-module. The assembled system is not FIPS-validated as a whole — individual crypto modules are. Document which module handles which crypto operation.

## Hardware Fleet

### Per-site deployment:

| Role | Hardware | Count | Responsibilities |
|---|---|---|---|
| **Command Post (CP)** | NVIDIA Jetson AGX Orin | 1 | Sensor fusion, Zenoh storage, SPIRE server, Kanidm replica, NORM sender, site-local relay, WireGuard gateway |
| **Mission Functional Terminal (MFT)** | Raspberry Pi 5 (8GB) | 5 | Sensor interface, Zenoh pub/sub endpoint, SPIRE agent, lightweight compute, operator display |
| **Cloud Relay / Identity** | AWS EC2 (t3.small or ARM equivalent) | 1 per region | iroh relay, SPIRE server (cloud trust domain), Kanidm primary, Zenoh cloud router, monitoring |

### Dev/test:

| Role | Hardware | Count |
|---|---|---|
| **Dev node** | CWWK x86 mini PC | 1 |
| **Simulated MFTs** | Docker containers or VMs | N |

### Operating Systems:
- AGX Orin: JetPack 6.x (Ubuntu 22.04 based) or Ubuntu 24.04 if Orin supports it
- Raspberry Pi 5: Ubuntu 24.04 LTS (arm64)
- Cloud: Ubuntu 24.04 LTS (x86_64 or arm64)
- CWWK dev: Ubuntu 24.04 LTS (x86_64)
- All nodes: LUKS full disk encryption. DISA STIG hardening (Phase 2).

## Network Transports

Nodes connect through whatever physical transport is available. The stack is transport-agnostic — iroh handles discovery and NAT traversal regardless of underlay.

| Transport | Characteristics | Where used |
|---|---|---|
| **Ethernet LAN** | Reliable, low latency, high bandwidth | Site-internal: CP ↔ MFTs via switch |
| **WiFi (local AP)** | Moderate reliability, mobility | Site-internal: dismounted MFTs |
| **Star Shield (Starlink)** | High bandwidth, variable latency (20-60ms), CG-NAT, no port forwarding | Site-to-cloud, site-to-site via relay |
| **5G/LTE puck** | Moderate bandwidth, higher latency, CG-NAT | Backup WAN, mobile site connectivity |
| **Cross-site relay** | Via iroh cloud relays when direct P2P fails | Site-to-site when no direct path exists |

**NAT traversal:** Star Shield and 5G/LTE pucks are both CG-NAT. No port forwarding available. iroh handles this with QUIC holepunching. When holepunching fails (symmetric NAT on both sides), traffic falls back to the nearest iroh relay. This is automatic and transparent.

**LAN multicast:** NORM uses UDP multicast on the local LAN. Multicast does not cross NAT or WAN links. Each site's LAN is an independent multicast domain.

**Interface failover:** iroh's QUIC multipath support means a connection can migrate from WiFi to LTE to Ethernet without dropping. The application layer (Zenoh, meshd) sees a stable connection despite the underlying transport changing.

## Cloud Infrastructure (Optional, Active-Active)

Cloud services are additive. The system operates fully without them. When deployed, they are active-active across multiple regions. Each is independently functional. Loss of any region degrades nothing — other regions continue serving. Loss of all regions degrades nothing at the edge.

### Per region (deploy to as many regions as useful, 3 recommended when cloud is available)

```
Region (e.g., eu-central-1):
├── iroh relay server (stateless, t3.micro or equivalent)
│   ├── Assists NAT traversal / holepunching
│   └── Encrypted relay fallback when direct P2P fails
├── SPIRE Server (cloud trust domain)
│   ├── Issues certs for cloud workloads
│   └── Federates with site SPIRE servers
├── Kanidm instance (primary in one region, replicas in others)
│   └── Human operator identity (OAuth2/OIDC)
└── Zenoh router (cloud storage for cross-site data aggregation)
```

**DNS:** Use Cloudflare DNS for the domain. A records for each relay region. DNS round-robin for geographic distribution. Records are NOT proxied (no Cloudflare HTTP proxy — it breaks QUIC and gRPC).

**Why no load balancer:** iroh clients connect to specific relay nodes by their iroh public key, not by DNS hostname. DNS just helps initial discovery. Once a node knows its relay's key, it can find it through any discovery mechanism. A traditional L4/L7 load balancer in front of iroh relays adds a SPOF and breaks the end-to-end encryption model.

**Cost:** iroh relays are lightweight. A t3.micro ($8/month) handles thousands of connections. SPIRE servers need minimal compute. Total cloud cost for 3 regions should be under $100/month.

## Edge-First / Island Mode

Island mode is not a fallback. It is the primary operating assumption. Every site is designed to function indefinitely with no external connectivity. Cloud and cross-site links are enhancements that the system exploits when present.

**Baseline capability (no internet, no cloud, no cross-site links):**

1. **iroh discovers local peers via mDNS.** No server, no DNS, no internet. Peers on the same LAN find each other automatically.
2. **WireGuard overlay operates.** `meshd` configures peer entries from iroh discovery. SSH and IP-based tools work over overlay IPs.
3. **Zenoh pub/sub and storage operate locally.** All site-internal data flows — sensor to fusion, commands to effectors — work entirely on the LAN.
4. **SPIRE issues and rotates certs.** The site's SPIRE server is a sovereign CA. It needs nothing external.
5. **Kanidm authenticates operators.** The site's Kanidm instance is a fully functional identity provider. OAuth2/OIDC works on the LAN.
6. **NORM multicast operates.** Multicast is LAN-only by design.

**What cloud and cross-site connectivity add (when available):**
- Cross-site Zenoh data exchange (sensor feeds, fused tracks, commands between sites)
- iroh relay for NAT traversal between sites behind CG-NAT
- SPIRE federation (cross-site mTLS between different trust domains)
- Kanidm replication (user/group changes sync across sites)
- Cloud Zenoh storage (cross-site data aggregation and monitoring)

**What happens when connectivity is lost after it existed:**
- Cross-site data stops flowing. Each site continues independently.
- Cloud services become unreachable. No impact on edge operations.
- SPIRE federation bundles stop updating. Existing trust still works — certs from previously-federated domains remain valid until expiry.

**What recovers automatically when connectivity returns:**
- iroh re-establishes relay paths to cloud and other sites
- `meshd` updates WireGuard peer endpoints for newly-reachable peers
- Zenoh storages anti-entropy: sites exchange deltas, merge divergent data
- SPIRE servers re-exchange trust bundles
- Kanidm syncs latest user/group changes

**No manual intervention required.** The system heals itself.

## Enrollment and Provisioning

### Pre-deployment (no internet required):

Everything below happens on a LAN with no external connectivity. Cloud steps are additive — do them if you have internet, skip them if you don't.

1. **Generate root CA** on air-gapped machine. Sign intermediate CA certs for each SPIRE server (one per site, optionally one for cloud).
2. **Bootstrap site SPIRE server** on the CP (AGX Orin). Install intermediate CA cert. This is a fully functional CA — it issues certs, attests workloads, and rotates keys with no upstream dependency.
3. **Bootstrap site Kanidm** on the CP. Create operator accounts and groups locally. This is a fully functional identity provider — OAuth2/OIDC works on the LAN.
4. **Generate iroh keypair** for each node. Record the public key (this is the node's permanent identity).
5. **Generate WireGuard keypair** for each node. The public key is published via iroh.
6. **Create node provisioning bundle** containing:
   - iroh keypair
   - WireGuard keypair
   - SPIRE join token (one-time use, expires after first attestation)
   - List of known peer iroh public keys (for LAN discovery bootstrapping)
   - iroh relay public keys and hostnames (if cloud relays exist; optional)
   - Site-specific Zenoh configuration (topics, storage role)
   - NORM multicast group configuration
7. **Flash each node** with the provisioning bundle + OS image.
8. **Test enrollment** on a local LAN (all nodes on same switch, no internet). Verify:
   - iroh peers discover each other via mDNS
   - WireGuard overlay comes up (ping overlay IPs)
   - Zenoh pub/sub works
   - SPIRE issues certs
   - Kanidm authenticates test operators

**If cloud is available (additive, not required):**
- Deploy cloud SPIRE server. Federate with site SPIRE servers (exchange trust bundles).
- Deploy cloud Kanidm primary. Configure site Kanidm as replica (syncs when connected, operates independently when not).
- Deploy iroh relay servers. Add relay public keys to node provisioning bundles for cross-NAT connectivity.
- Deploy cloud Zenoh router for cross-site data aggregation.

### Field enrollment (standard, not emergency):

Enrolling new devices locally at a site is a first-class capability, not a fallback:

1. The site's CP (AGX Orin) runs the provisioning tool. It generates a new iroh keypair, WireGuard keypair, and SPIRE join token — all locally, no external connectivity.
2. The new device is physically connected to the site's LAN (ethernet or local WiFi).
3. The provisioning tool pushes the bundle to the new device via a local HTTPS endpoint (mTLS with SPIRE cert).
4. The device enrolls against the local SPIRE server, discovers peers via iroh mDNS, and meshd configures WireGuard. Fully operational in minutes.
5. If cloud connectivity exists or arrives later, the locally-enrolled device becomes visible to cloud services and other sites via normal federation. No re-enrollment needed.

## Implementation Phases

### Phase 0: Skeleton and Dev Environment

**Goal:** Repo structure, build system, dev environment, single-node smoke test.

- Initialize Rust workspace with the following crates:
  - `meshd` — the iroh-WireGuard bridge daemon
  - `provision` — node provisioning tool (generates bundles, flashes config)
  - `fabric-cli` — operator CLI for mesh status, debugging, manual overrides
- Set up cross-compilation for aarch64 (AGX Orin, RPi)
- Docker Compose dev environment that simulates a 3-node site (1 CP + 2 MFT containers)
- CI pipeline: build, test, cross-compile, lint
- CLAUDE.md with project conventions

**Deliverable:** `cargo build` succeeds. Dev containers start. Single node runs iroh endpoint and creates a WireGuard interface.

### Phase 1: Connection Layer (iroh + WireGuard)

**Goal:** Nodes discover each other and establish encrypted IP-layer connectivity.

- `meshd` daemon: iroh endpoint + WireGuard management
  - mDNS local discovery (LAN peers find each other without config)
  - iroh relay connection (cloud relay for cross-NAT connectivity)
  - WireGuard peer management (add/remove/update peers based on iroh events)
  - Deterministic overlay IP assignment from iroh public key
  - Health checking: detect peer liveness, remove stale WireGuard entries
- iroh relay server deployment (Terraform for AWS infrastructure)
  - 3 regions: eu-central-1 (Frankfurt), ap-south-1 (Mumbai), il-central-1 (Tel Aviv)
  - Security groups: UDP 3478 (QUIC), TCP 443 (HTTPS for relay HTTP endpoint)
  - Elastic IPs for stable addresses
  - Cloudflare DNS A records
- Integration tests:
  - Two nodes on same LAN discover each other, WireGuard overlay comes up, SSH works over overlay IP
  - Two nodes behind NAT (simulated with iptables) connect via relay, WireGuard overlay comes up
  - Node roams (changes IP), iroh detects, WireGuard endpoint updates, connection recovers
  - Relay goes down, LAN nodes continue operating via mDNS

**Deliverable:** N nodes can mesh, assign overlay IPs, and pass IP traffic (SSH, ping) over WireGuard, with iroh handling all discovery and NAT traversal.

### Phase 2: Identity Layer (SPIRE/SPIFFE + Kanidm)

**Goal:** Every connection is mutually authenticated. No implicit trust.

- SPIRE Server deployment (Docker container, one per site, one cloud)
  - Root CA ceremony (air-gapped, document the procedure)
  - Intermediate CA per SPIRE server
  - Node attestor: use `x509pop` (X.509 Proof of Possession) or `join_token` for initial attestation
  - Workload attestor: `unix` (PID, UID, GID) for Linux workloads
- SPIRE Agent on every node
  - Automated SVID (X.509 certificate) retrieval and rotation
  - SPIFFE Workload API socket for local workloads to request certs
- Federation between SPIRE servers
  - Configure bundle endpoints for each trust domain
  - Test: workload at site alpha authenticates to workload at site bravo using federated trust
- `meshd` integration:
  - Before adding a WireGuard peer, validate that the peer's iroh-delivered SPIRE SVID chains to a trusted CA
  - Reject peers with expired or revoked certs
- Kanidm deployment
  - Cloud primary instance
  - Per-site replica
  - OAuth2/OIDC configuration for web-based operator tools
  - User/group provisioning automation
- Integration tests:
  - Workload-to-workload mTLS using SPIRE-issued certs
  - Federation: cross-site mTLS between different trust domains
  - SPIRE server down at site: agent continues with cached certs until expiry
  - Kanidm offline: local replica authenticates operators
  - Cert rotation: certs rotate automatically without disruption

**Deliverable:** Zero-trust mTLS everywhere. Every process proves its identity. Human operators authenticate via Kanidm. Federation works across trust domains.

### Phase 3: Data Layer (Zenoh + NORM)

**Goal:** Structured pub/sub data fabric with reliable multicast for bulk distribution.

- Zenoh router on CP nodes (AGX Orin)
  - Storage plugin configured for site-local data
  - TLS transport with SPIRE-issued certs
  - Multicast scouting on LAN for peer discovery
  - TCP/QUIC for cross-site links (over iroh connections)
- Zenoh client on MFT nodes (RPi)
  - Pub/sub endpoints (publish sensor data, subscribe to commands)
  - Lightweight — no storage, no routing
- Zenoh cloud router
  - Aggregation storage for cross-site data
  - Accessible from all sites via iroh
- Topic namespace implementation per the schema defined above
- NORM integration:
  - NORM sender on CP node for bulk LAN multicast (firmware updates, map tiles, large data sets)
  - NORM receiver on all MFT nodes
  - Configurable FEC ratio per transfer
  - CLI tool to initiate NORM transfers (`fabric-cli norm push <file> --fec-ratio 0.2`)
- Zenoh-plugin-dds (if DDS interop is needed now; defer if not)
- Integration tests:
  - Pub/sub between CP and MFTs on LAN
  - Pub/sub between two sites via iroh relay
  - Storage anti-entropy: partition two sites, publish different data to each, reconnect, verify merge
  - Late joiner: new subscriber comes online, receives historical data
  - NORM: push 100MB file to 5 MFTs via LAN multicast, verify integrity
  - NORM under loss: simulate 5% packet loss, verify FEC recovery

**Deliverable:** Sensor data flows from MFTs to CP, fused data flows between sites, partitioned sites reconcile automatically, bulk distribution works via multicast.

### Phase 4: Hardening and Operations

**Goal:** Production-ready deployment with monitoring, security hardening, and operational tooling.

- DISA STIG hardening for Ubuntu (automated via Ansible or shell scripts)
- FIPS 140-3: switch crypto modules to AWS-LC or OpenSSL FIPS
- PQC: integrate Rosenpass for WireGuard, ML-KEM/ML-DSA in cert chain
- Monitoring:
  - Prometheus metrics from meshd (peer count, connection type, latency)
  - Zenoh diagnostics (pub/sub rates, storage sizes, reconciliation events)
  - SPIRE health (cert issuance rate, expiry warnings)
  - Local dashboard on CP (Grafana or custom lightweight UI)
- Ansible playbooks for fleet deployment (flash, configure, verify)
- `fabric-cli` operational commands:
  - `fabric-cli status` — mesh overview (peers, connections, overlay IPs)
  - `fabric-cli peers` — detailed peer list with latency, connection type (direct/relay)
  - `fabric-cli certs` — SPIRE cert status for all local workloads
  - `fabric-cli zenoh topics` — active topics and subscriber counts
  - `fabric-cli provision <node-id>` — generate provisioning bundle for new node
- Backup/restore procedures for SPIRE CA keys, Kanidm database
- Runbooks: common failures, recovery procedures, island mode activation checklist

**Deliverable:** Hardened, monitored, operable production system with fleet management tooling.

## Repository Structure

```
/
├── CLAUDE.md                   # Project conventions for AI-assisted development
├── Cargo.toml                  # Rust workspace root
├── crates/
│   ├── meshd/                  # iroh-WireGuard bridge daemon
│   │   ├── src/
│   │   │   ├── main.rs
│   │   │   ├── iroh.rs         # iroh endpoint management
│   │   │   ├── wireguard.rs    # WireGuard interface management (wg set)
│   │   │   ├── discovery.rs    # mDNS + relay discovery logic
│   │   │   ├── overlay_ip.rs   # Deterministic IP assignment
│   │   │   └── spire.rs        # SPIRE SVID validation
│   │   └── Cargo.toml
│   ├── provision/              # Node provisioning tool
│   │   ├── src/
│   │   └── Cargo.toml
│   └── fabric-cli/             # Operator CLI
│       ├── src/
│       └── Cargo.toml
├── ops/
│   ├── ansible/                # Fleet deployment playbooks
│   │   ├── site.yml            # Full site deployment
│   │   ├── roles/
│   │   │   ├── base/           # OS hardening, packages, LUKS
│   │   │   ├── meshd/          # meshd daemon deployment
│   │   │   ├── spire-server/   # SPIRE server setup
│   │   │   ├── spire-agent/    # SPIRE agent setup
│   │   │   ├── kanidm/         # Kanidm deployment
│   │   │   ├── zenoh/          # Zenoh router/client config
│   │   │   └── norm/           # NORM sender/receiver setup
│   │   └── inventory/
│   │       ├── dev.yml         # Dev environment (CWWK + containers)
│   │       └── site-alpha.yml  # Example site inventory
│   ├── docker/
│   │   ├── dev-compose.yml     # Dev environment: 3 simulated nodes
│   │   ├── Dockerfile.meshd    # meshd container image
│   │   ├── Dockerfile.spire    # SPIRE server image
│   │   └── Dockerfile.zenoh    # Zenoh router image
│   └── scripts/
│       ├── root-ca-ceremony.sh # Air-gapped root CA generation
│       ├── provision-node.sh   # Generate and flash node bundle
│       └── island-test.sh      # Automated island mode test
├── terraform/
│   ├── main.tf                 # Multi-region cloud infrastructure
│   ├── modules/
│   │   ├── relay/              # iroh relay server (per region)
│   │   ├── spire-cloud/        # Cloud SPIRE server
│   │   ├── kanidm-cloud/       # Cloud Kanidm instance
│   │   └── zenoh-cloud/        # Cloud Zenoh router
│   ├── variables.tf
│   ├── outputs.tf
│   └── backend.tf
├── config/
│   ├── zenoh/
│   │   ├── cp-router.json5     # Zenoh config for Command Post
│   │   ├── mft-client.json5    # Zenoh config for MFT
│   │   └── cloud-router.json5  # Zenoh config for cloud aggregator
│   ├── spire/
│   │   ├── server.conf         # SPIRE server config template
│   │   └── agent.conf          # SPIRE agent config template
│   ├── kanidm/
│   │   └── server.toml         # Kanidm config template
│   └── norm/
│       └── defaults.conf       # NORM FEC and multicast defaults
├── docs/
│   ├── architecture.md         # System architecture (this prompt, refined)
│   ├── enrollment.md           # Provisioning and enrollment procedures
│   ├── island-mode.md          # Disconnected operation guide
│   ├── security.md             # Threat model, crypto details, FIPS/PQC
│   ├── runbooks/               # Operational runbooks
│   └── adr/                    # Architecture Decision Records
│       ├── 001-iroh-over-nebula.md
│       ├── 002-wireguard-bootstrap-via-iroh.md
│       ├── 003-spire-per-site-federation.md
│       ├── 004-kanidm-over-ory-dex.md
│       ├── 005-norm-for-lan-multicast.md
│       └── 006-deterministic-overlay-ip.md
└── tests/
    ├── integration/
    │   ├── lan_discovery.rs    # mDNS + WireGuard on LAN
    │   ├── relay_fallback.rs   # NAT traversal via relay
    │   ├── island_mode.rs      # Full island mode test
    │   ├── partition_heal.rs   # Zenoh storage reconciliation
    │   └── cert_rotation.rs    # SPIRE cert lifecycle
    └── e2e/
        └── full_site.rs        # End-to-end site deployment test
```

## Key Technical Decisions (ADRs)

These decisions are made. Do not revisit them without explicit instruction.

1. **iroh over Nebula/Netbird/Headscale** for the connection layer. Reason: no central control plane, QUIC-native NAT traversal, mDNS local discovery, QUIC multipath for transport failover. Nebula's development is stagnant and its mobile clients are crippled. Netbird/Headscale are single-server architectures with no federation story.

2. **Raw WireGuard bootstrapped by iroh** for IP overlay. Reason: WireGuard is a kernel module that does one thing perfectly. iroh handles the hard part (discovery, NAT traversal). The bridge daemon (`meshd`) is ~500 lines of glue. This avoids depending on any mesh management product.

3. **SPIRE/SPIFFE per-site with federation** for workload identity. Reason: each site is sovereign (no split-brain), federation is built-in (not bolted on), cert issuance continues when disconnected. This is what SPIRE was designed for.

4. **Kanidm over Ory/Dex** for human identity. Reason: single binary, built-in replication, offline-capable, security-first design. Ory is 4+ services. Dex is a federation proxy, not an identity store. If no web UIs exist, Kanidm can be deferred entirely.

5. **NORM for LAN multicast, not Zenoh.** Reason: NORM is purpose-built for reliable multicast with FEC. Zenoh is pub/sub with storage semantics. Different tools for different jobs. NORM handles bulk distribution on LAN. Zenoh handles structured data across the fabric.

6. **Deterministic overlay IPs from iroh public key.** Reason: no allocation server, no DHCP, no state. Every node derives the same IP for the same key. Collision probability in a /10 space (~4M addresses) is negligible for fleets under 10K nodes.

## What NOT to Build

- A web dashboard (defer until there's something worth displaying in a browser)
- A custom messaging protocol (Zenoh does this)
- A custom identity system (SPIRE + Kanidm do this)
- A custom NAT traversal implementation (iroh does this)
- A custom reliable multicast (NORM does this)
- Anything that duplicates functionality already provided by the component stack
- Backward compatibility with the previous Netbird-based architecture (this is a clean break)

## Constraints

- **Bahrain (me-south-1) and UAE (me-central-1) AWS regions are destroyed.** Do not deploy infrastructure there.
- **Star Shield (Starlink) does not support port forwarding.** All connections from behind Star Shield must be outbound-initiated.
- **macOS development machines may be used for local testing** but are not deployment targets. All deployment targets are Linux (Ubuntu 24.04 on x86_64 or arm64).
- **All secrets stay out of git.** Use direnv (`.envrc` + `.envrc.local`, both gitignored) for local development. Use AWS Secrets Manager or Vault for production secrets.
- **AWS credentials via `aws-vault`, never in env files or committed to git.**

## Success Criteria

The system is complete when:

1. A 3-node site (1 CP + 2 MFTs) on a LAN switch can mesh, exchange Zenoh pub/sub data, and authenticate all connections — with no internet.
2. Two sites connected via Star Shield and iroh cloud relay can exchange Zenoh data and federate SPIRE trust.
3. Disconnect one site from the internet. Both sites continue operating independently. Reconnect. Zenoh storages reconcile. No manual intervention.
4. A new node can be provisioned and enrolled locally at a disconnected site by the CP's provisioning tool.
5. An operator can SSH to any node via its WireGuard overlay IP from any other node in the mesh.
6. NORM can push a 500MB file to all MFTs on a LAN simultaneously with FEC recovery under 5% packet loss.
7. All inter-workload connections use mTLS with SPIRE-issued certificates. No exceptions.
