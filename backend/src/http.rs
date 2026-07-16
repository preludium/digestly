//! HTTP layer: router, health endpoint, static SPA serving (prompt.md §10).
//!
//! `/api/*` routes are matched first; everything else is handled by the SPA fallback,
//! which serves a real static file when one exists and otherwise returns `index.html`
//! with 200 so client-side (React Router) deep links survive refresh/back.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{FromRef, Request, State};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::cookie::Key;
use serde_json::json;
use sqlx::SqlitePool;
use tower::ServiceExt;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

/// Shared application state handed to handlers. Grows in later phases.
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    /// Directory of built frontend assets.
    pub static_dir: PathBuf,
    /// `index.html` contents, read once at startup (served for SPA routes).
    pub index_html: Arc<str>,
    /// Key for signing session cookies (derived from `SECRET_KEY`).
    pub key: Key,
    /// 32-byte AEAD key for encrypting secrets at rest (provider keys, ntfy token). Derived from
    /// `SECRET_KEY` via SHA-256 (prompt.md §6, §11). Never returned or logged.
    pub enc_key: [u8; 32],
    /// Shared HTTP client for ingestion + discovery (Phase 3).
    pub http_client: reqwest::Client,
    /// Wakes the ingestion scheduler for refresh-now / new subscriptions (Phase 3).
    pub ingest_trigger: crate::ingest::IngestTrigger,
    /// Live event bus + ingest-run registry. Backs `GET /api/events` (SSE) and lets the
    /// scheduler tell a browser when *its* "Ingest now" actually finished.
    pub events: crate::events::Events,
    /// WebAuthn Relying Party (passkeys, S1). `None` if RP config is invalid → passkey endpoints
    /// report "not enabled" and the UI hides the button (the app stays fully usable via password).
    pub webauthn: Option<std::sync::Arc<webauthn_rs::Webauthn>>,
    /// Short-lived, in-process passkey ceremony state (options → verify).
    pub passkey_ceremonies: crate::auth::passkey::CeremonyStore,
    /// OAuth import client credentials + redirect base (YouTube/Reddit, S4). Providers with no
    /// credentials are hidden in the UI and their endpoints report "not configured".
    pub oauth: std::sync::Arc<crate::oauth::OAuthSettings>,
    /// Short-lived, in-process OAuth authorization state (CSRF) → (user, provider).
    pub oauth_states: crate::oauth::OAuthStates,
}

// Lets `SignedCookieJar` pull the signing key out of `AppState`.
impl FromRef<AppState> for Key {
    fn from_ref(state: &AppState) -> Self {
        state.key.clone()
    }
}

/// Build the full axum router: API routes + static SPA fallback + middleware.
pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .merge(crate::routes::api_router())
        .layer(middleware::from_fn(log_500));

    // CORS is only for the Tauri origin (the web build is same-origin, served by this binary).
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
        .allow_credentials(true)
        .allow_origin(tauri_origins());

    // Never compress the SSE stream: the encoder holds bytes back to fill a block, which
    // would delay - or swallow - live events until the connection closes.
    let compression = CompressionLayer::new().compress_when(
        DefaultPredicate::new().and(NotForContentType::const_new("text/event-stream")),
    );

    Router::new()
        .nest("/api", api)
        .fallback(spa_fallback)
        .layer(TraceLayer::new_for_http())
        .layer(compression)
        .layer(cors)
        .with_state(state)
}

/// Log server errors with method + URI context. The full error chain is carried
/// through `InternalError` inserted by `AppError::into_response`.
async fn log_500(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let uri = request.uri().to_string();
    let response = next.run(request).await;
    if response.status().is_server_error() {
        if let Some(crate::error::InternalError(e)) = response.extensions().get() {
            tracing::error!(%method, uri = %uri, error = ?e, "internal error");
        } else {
            tracing::error!(%method, uri = %uri, status = %response.status(), "request failed");
        }
    }
    response
}

/// Allowed cross-origin values used by the Tauri Android/desktop shell.
fn tauri_origins() -> Vec<HeaderValue> {
    [
        "tauri://localhost",
        "http://tauri.localhost",
        "https://tauri.localhost",
    ]
    .into_iter()
    .filter_map(|o| HeaderValue::from_str(o).ok())
    .collect()
}

/// Serve a real static file if it exists; otherwise return `index.html` (200) for GET
/// so SPA routes work. Non-GET misses stay 404.
async fn spa_fallback(State(state): State<AppState>, req: Request) -> Response {
    // Unknown API routes are a real 404 (JSON), never the SPA shell.
    if req.uri().path().starts_with("/api") {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response();
    }
    let is_get = req.method() == Method::GET;
    let serve = ServeDir::new(&state.static_dir);
    // ServeDir is infallible.
    let res = serve.oneshot(req).await.into_response();
    if res.status() == StatusCode::NOT_FOUND && is_get {
        (StatusCode::OK, Html(state.index_html.to_string())).into_response()
    } else {
        res
    }
}

/// `GET /api/health` → `{status, version, db_ok}`. 200 when the DB responds, 503 otherwise.
async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let db_ok = crate::db::ping(&state.pool).await;
    let body = Json(json!({
        "status": if db_ok { "ok" } else { "degraded" },
        "version": env!("CARGO_PKG_VERSION"),
        "db_ok": db_ok,
    }));
    let code = if db_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (code, body)
}
