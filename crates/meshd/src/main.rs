use anyhow::Result;
use clap::Parser;
use iroh::endpoint::presets;
use iroh::protocol::Router;
use iroh::{Endpoint, RelayMode, SecretKey};
use meshd::discovery;
use meshd::overlay_ip::overlay_ip_from_id;
use meshd::peer::{PeerHandshake, PeerTable};
use meshd::protocol::{MeshProtocol, MESH_ALPN};
use meshd::wireguard::WgInterface;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(name = "meshd", about = "iroh-WireGuard bridge daemon")]
struct Args {
    /// WireGuard interface name
    #[arg(long, default_value = "wg0")]
    wg_interface: String,

    /// WireGuard listen port
    #[arg(long, default_value = "51820")]
    wg_port: u16,

    /// Path to persistent iroh secret key file
    #[arg(long, default_value = "/var/lib/meshd/secret.key")]
    key_file: PathBuf,

    /// Disable cloud relay (island mode)
    #[arg(long)]
    no_relay: bool,

    /// Custom relay URLs (comma-separated)
    #[arg(long, value_delimiter = ',')]
    relay_urls: Vec<String>,

    /// Log level
    #[arg(long, default_value = "info")]
    log_level: String,
}

/// Load or generate a persistent secret key.
fn load_or_generate_key(path: &PathBuf) -> Result<SecretKey> {
    if path.exists() {
        let bytes = std::fs::read(path)?;
        let key = SecretKey::from_bytes(&bytes.try_into().map_err(|_| {
            anyhow::anyhow!("invalid key file length")
        })?);
        info!(path = %path.display(), "loaded existing secret key");
        Ok(key)
    } else {
        let key = SecretKey::generate(&mut rand::rng());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, key.to_bytes())?;
        info!(path = %path.display(), "generated new secret key");
        Ok(key)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| args.log_level.parse().unwrap()),
        )
        .init();

    info!("meshd starting");

    // 1. Load or generate iroh identity
    let secret_key = load_or_generate_key(&args.key_file)?;
    let endpoint_id = secret_key.public();
    let overlay_ip = overlay_ip_from_id(&endpoint_id);
    info!(
        id = %endpoint_id,
        overlay_ip = %overlay_ip,
        "node identity established"
    );

    // 2. Configure relay mode
    let relay_mode = if args.no_relay {
        RelayMode::Disabled
    } else if !args.relay_urls.is_empty() {
        let urls: Vec<_> = args
            .relay_urls
            .iter()
            .filter_map(|u| u.parse().ok())
            .collect();
        RelayMode::custom(urls)
    } else {
        RelayMode::Default
    };

    // 3. Create iroh endpoint
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(secret_key)
        .alpns(vec![MESH_ALPN.to_vec()])
        .relay_mode(relay_mode)
        .bind()
        .await?;

    info!("waiting for endpoint to come online...");
    endpoint.online().await;

    let addr = endpoint.addr();
    info!(id = %endpoint.id(), addr = ?addr, "iroh endpoint online");

    // 4. Set up WireGuard interface
    let wg = match WgInterface::setup(&args.wg_interface, args.wg_port, overlay_ip) {
        Ok(wg) => {
            info!(
                interface = %wg.name,
                wg_pubkey = %wg.keypair.public_key,
                overlay_ip = %wg.overlay_ip,
                "WireGuard interface configured"
            );
            Some(wg)
        }
        Err(e) => {
            warn!(error = %e, "WireGuard setup failed (continuing without WG)");
            None
        }
    };

    let wg_pubkey = wg
        .as_ref()
        .map(|w| w.keypair.public_key.clone())
        .unwrap_or_else(|| "none".to_string());

    let wg = Arc::new(RwLock::new(wg));

    // 5. Build protocol handler
    let peers = PeerTable::new();
    let local_handshake = PeerHandshake {
        wg_pubkey,
        overlay_ip,
        wg_endpoint: None, // Will be updated when we know our external endpoint
    };

    let mesh_protocol = MeshProtocol::new(peers.clone(), local_handshake, wg.clone());

    // 6. Start the iroh router (accepts incoming connections)
    let endpoint = Arc::new(endpoint);
    let router = Router::builder((*endpoint).clone())
        .accept(MESH_ALPN, mesh_protocol.clone())
        .spawn();

    info!("mesh router started, accepting connections");

    // 7. Start mDNS discovery
    let ep_clone = endpoint.clone();
    let proto_clone = mesh_protocol.clone();
    let mdns_handle = tokio::spawn(async move {
        if let Err(e) = discovery::run_mdns_discovery(ep_clone, proto_clone).await {
            warn!(error = %e, "mDNS discovery ended");
        }
    });

    // 8. Status logging loop
    let peers_clone = peers.clone();
    let status_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let count = peers_clone.count().await;
            let peers_list = peers_clone.list().await;
            info!(
                peer_count = count,
                "mesh status"
            );
            for p in &peers_list {
                info!(
                    peer = %p.endpoint_id,
                    ip = %p.overlay_ip,
                    connected = p.connected,
                    "  peer"
                );
            }
        }
    });

    // 9. Wait for shutdown
    info!("meshd running — press Ctrl+C to stop");
    tokio::signal::ctrl_c().await?;
    info!("shutting down...");

    // Cleanup
    mdns_handle.abort();
    status_handle.abort();
    router.shutdown().await?;

    // Teardown WireGuard
    let wg_guard = wg.read().await;
    if let Some(ref wg_iface) = *wg_guard {
        wg_iface.teardown().ok();
    }

    info!("meshd stopped");
    Ok(())
}
