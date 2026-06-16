//! `nexus-memory-hub` — the standalone central sync server.
//!
//! Run it on a host every Nexus instance can reach and point clients at it with
//! a shared secret:
//!
//! ```text
//! SYNC_SECRET=$(openssl rand -hex 32) \
//!   nexus-memory-hub --bind 0.0.0.0:8765 --db /var/lib/nexus-hub/hub.sqlite3
//! ```

#![warn(clippy::pedantic)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use nexus_memory_hub::{router, AppState, HubStore};

/// Central sync hub for Nexus memory.
#[derive(Parser, Debug)]
#[command(name = "nexus-memory-hub", version, about)]
struct Args {
    /// Address to bind, `ip:port`.
    #[arg(long, default_value = "127.0.0.1:8765")]
    bind: String,
    /// Path to the hub's SQLite database (created if absent).
    #[arg(long, default_value = "hub.sqlite3")]
    db: PathBuf,
    /// Shared bearer secret every client must present. Read from `SYNC_SECRET`
    /// if the flag is omitted.
    #[arg(long, env = "SYNC_SECRET")]
    secret: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    if args.secret.trim().is_empty() {
        anyhow::bail!("SYNC_SECRET must be set and non-empty");
    }

    let store = HubStore::open(&args.db)?;
    let state = AppState {
        store,
        secret: Arc::new(args.secret),
    };

    let addr: SocketAddr = args
        .bind
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid --bind {:?}: {e}", args.bind))?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, db = %args.db.display(), "nexus-memory-hub listening");
    axum::serve(listener, router(state)).await?;
    Ok(())
}
