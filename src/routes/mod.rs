//! REST API routes (prompt.md §10). Phase 2 covers auth, account, admin user-management,
//! and a read-only per-user `categories` endpoint (scoping anchor). Later phases merge more.

pub mod admin;
pub mod ai;
pub mod auth;
pub mod categories;
pub mod digest;
pub mod feeds;
pub mod items;
pub mod me;
pub mod notifications;
pub mod oauth;
pub mod opml;
pub mod passkeys;
pub mod settings;

use axum::Router;

use crate::http::AppState;

/// Compose the `/api` sub-routers (mounted under `/api` by `http::router`).
pub fn api_router() -> Router<AppState> {
    Router::new()
        .merge(auth::routes())
        .merge(me::routes())
        .merge(admin::routes())
        .merge(categories::routes())
        .merge(feeds::routes())
        .merge(items::routes())
        .merge(ai::routes())
        .merge(notifications::routes())
        .merge(digest::routes())
        .merge(settings::routes())
        .merge(opml::routes())
        .merge(passkeys::routes())
        .merge(oauth::routes())
}

/// Shared user shape returned by auth/me endpoints.
#[derive(serde::Serialize)]
pub struct UserDto {
    pub id: i64,
    pub username: String,
    pub role: crate::auth::Role,
}
