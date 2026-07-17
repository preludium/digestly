//! Digestly server entrypoint (Phase 1: boot, migrate, serve, shut down cleanly).

mod ai;
mod auth;
mod config;
mod db;
mod digest;
mod error;
mod events;
mod healthcheck;
mod http;
mod ingest;
mod maintenance;
mod notify;
mod oauth;
mod opml;
mod query;
mod routes;
mod seed;
mod seed_demo;
mod settings;

#[cfg(test)]
mod isolation_tests;

use std::sync::Arc;

use anyhow::{Context, Result};
use axum_extra::extract::cookie::Key;
use sha2::{Digest, Sha256, Sha512};
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::Notify;
use tracing::{info, warn};
use tracing_subscriber::{fmt, EnvFilter};

use crate::config::Config;
use crate::http::AppState;

#[tokio::main]
async fn main() -> Result<()> {
    // Container HEALTHCHECK path: probe the running server and exit, no env required.
    if std::env::args().any(|a| a == "--healthcheck") {
        return healthcheck::run().await;
    }

    init_tracing();

    // Test-mode/seed command (§13): ingest bundled fixtures offline + print a sample digest.
    if std::env::args().any(|a| a == "--seed") {
        return seed_demo::run().await;
    }

    // Load a local `.env` if present so `cargo run` works without exporting vars by hand.
    // Existing env is never overridden, and there is no `.env` in the Docker image (its vars
    // come from compose `env_file`), so this is a no-op in the container.
    let _ = dotenvy::dotenv();

    // Fail fast on missing/invalid required env (§12).
    let cfg = Config::from_env().context("invalid configuration")?;

    // Ensure the data directory exists before opening the DB.
    std::fs::create_dir_all(&cfg.data_dir)
        .with_context(|| format!("cannot create DATA_DIR {}", cfg.data_dir.display()))?;

    let pool = db::connect(&cfg.db_path()).await?;
    db::migrate(&pool).await?;
    info!(db = %cfg.db_path().display(), "database ready, migrations applied");

    // Ensure the built-in admin + global defaults exist (re-syncs admin hash from env).
    auth::bootstrap::run(&pool, &cfg.admin_password).await?;

    // Read the SPA entry once; served for client-side routes. Empty if not built yet.
    let index_html = std::fs::read_to_string(cfg.static_dir.join("index.html"))
        .unwrap_or_default()
        .into();

    // Derive a stable 64-byte cookie-signing key from SECRET_KEY.
    let key = Key::from(&Sha512::digest(cfg.secret_key.as_bytes()));
    // Derive a 32-byte AEAD key from SECRET_KEY for encrypting secrets at rest (§6, §11).
    let enc_key: [u8; 32] = Sha256::digest(cfg.secret_key.as_bytes()).into();

    // Shared ingestion engine (§4): one HTTP client + a trigger the API uses to poke the
    // scheduler for refresh-now / new subscriptions.
    let http_client = ingest::fetch::build_client();
    let ingest_trigger: ingest::IngestTrigger = Arc::new(Notify::new());
    // Live push channel to open browser tabs: the scheduler reports every completed feed
    // poll here, which is how an "Ingest now" run reaches the user's toast and refreshes their feed.
    let events = events::EventBus::new();
    // Notified by the scheduler whenever a YouTube feed poll stores new items, so the background
    // transcript worker fetches captions shortly after ingest instead of on its own idle tick.
    let new_video_trigger: ai::transcript_worker::TranscriptTrigger = Arc::new(Notify::new());
    // The scheduler gets the AEAD key so it can decrypt per-user ntfy tokens for the throttled
    // feed-health notifications it fires on healthy→failing/disabled transitions (§7a, §11).
    let scheduler = ingest::spawn(
        pool.clone(),
        http_client.clone(),
        enc_key,
        cfg.reddit_oauth.clone(),
        ingest_trigger.clone(),
        new_video_trigger.clone(),
        events.clone(),
    );
    let transcript_worker =
        ai::transcript_worker::spawn(pool.clone(), http_client.clone(), new_video_trigger);

    // Digest engine (§7): a global cron drives per-user, category-grouped, AI-summarized digests
    // pushed to each user's ntfy channel. Admins can also trigger it via POST /api/digest/run.
    let digest_scheduler = digest::spawn(pool.clone(), http_client.clone(), enc_key);

    // Periodic retention purge (§5, §8, §11) - keeps the SQLite file small; starred items survive.
    let maintenance = maintenance::spawn(pool.clone());

    // Passkeys / WebAuthn (S1): build the Relying Party from RP_ID/RP_ORIGIN. Never fatal - if the
    // origin is unparseable the app still boots and passkey endpoints report "not enabled".
    let webauthn = auth::passkey::build(&cfg.rp_id, &cfg.rp_origin, &cfg.rp_extra_origins);

    // OAuth import helpers (S4): per-provider client credentials (optional). The redirect URI is
    // derived from RP_ORIGIN. Providers with no credentials stay hidden in the UI.
    let oauth = std::sync::Arc::new(oauth::OAuthSettings {
        google: cfg.google_oauth.clone(),
        reddit: cfg.reddit_oauth.clone(),
        redirect_base: cfg.rp_origin.clone(),
    });

    let state = AppState {
        pool: pool.clone(),
        static_dir: cfg.static_dir.clone(),
        index_html,
        key,
        enc_key,
        http_client,
        ingest_trigger,
        events,
        webauthn,
        passkey_ceremonies: auth::passkey::CeremonyStore::new(),
        oauth,
        oauth_states: oauth::OAuthStates::new(),
    };
    let app = http::router(state);

    let listener = TcpListener::bind(&cfg.bind_addr)
        .await
        .with_context(|| format!("cannot bind {}", cfg.bind_addr))?;
    info!(addr = %cfg.bind_addr, static_dir = %cfg.static_dir.display(), "Digestly listening");

    // Graceful shutdown: stop accepting on SIGTERM/Ctrl-C, then stop the scheduler + close pool (§11).
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    scheduler.abort();
    transcript_worker.abort();
    digest_scheduler.abort();
    maintenance.abort();

    info!("shutting down: closing database pool");
    pool.close().await;
    Ok(())
}

fn init_tracing() {
    // Configurable via RUST_LOG; visible in `docker logs` (§12).
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,digestly=debug,tower_http=info"));
    fmt().with_env_filter(filter).init();
}

/// Resolves on SIGTERM (Docker stop) or Ctrl-C.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => warn!(error = %e, "failed to install SIGTERM handler"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("received Ctrl-C"),
        _ = terminate => info!("received SIGTERM"),
    }
}
