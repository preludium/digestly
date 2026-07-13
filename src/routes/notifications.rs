//! Per-user ntfy notification config + test (prompt.md §7a, §9.7 Notifications tab, §10).
//!
//! `GET /api/notifications` returns everything **except** the auth token (only `has_token`); `PUT`
//! accepts a write-only token; `POST /api/notifications/test` sends a test push (never echoes the
//! token). All scoped to the session user - never a client-supplied id (§11).

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::auth::extract::CurrentUser;
use crate::error::{ApiResult, AppError};
use crate::http::AppState;
use crate::notify::{self, TokenUpdate};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/notifications",
            get(get_notifications).put(put_notifications),
        )
        .route("/notifications/test", post(test_notifications))
}

#[derive(Serialize)]
struct NotificationsDto {
    ntfy_server_url: Option<String>,
    ntfy_topic: Option<String>,
    ntfy_priority: i64,
    notify_on_digest: bool,
    notify_on_feed_health: bool,
    has_token: bool,
}

impl From<notify::NotificationConfig> for NotificationsDto {
    fn from(c: notify::NotificationConfig) -> Self {
        NotificationsDto {
            ntfy_server_url: c.ntfy_server_url,
            ntfy_topic: c.ntfy_topic,
            ntfy_priority: c.ntfy_priority,
            notify_on_digest: c.notify_on_digest,
            notify_on_feed_health: c.notify_on_feed_health,
            has_token: c.has_token,
        }
    }
}

async fn get_notifications(
    user: CurrentUser,
    State(state): State<AppState>,
) -> ApiResult<Json<NotificationsDto>> {
    let cfg = notify::load(&state.pool, user.id)
        .await
        .map_err(AppError::Internal)?;
    Ok(Json(cfg.into()))
}

#[derive(Deserialize)]
struct PutNotifications {
    ntfy_server_url: Option<String>,
    ntfy_topic: Option<String>,
    ntfy_priority: Option<i64>,
    notify_on_digest: Option<bool>,
    notify_on_feed_health: Option<bool>,
    /// Write-only token. `null`/absent = keep existing; `""` = clear; a value = set (encrypted).
    #[serde(default, deserialize_with = "double_option")]
    auth_token: Option<Option<String>>,
}

async fn put_notifications(
    user: CurrentUser,
    State(state): State<AppState>,
    Json(body): Json<PutNotifications>,
) -> ApiResult<Json<NotificationsDto>> {
    // Preserve unspecified fields by starting from the current config.
    let current = notify::load(&state.pool, user.id)
        .await
        .map_err(AppError::Internal)?;

    let token = match body.auth_token {
        None => TokenUpdate::Keep,
        Some(None) => TokenUpdate::Keep,
        Some(Some(t)) if t.trim().is_empty() => TokenUpdate::Clear,
        Some(Some(t)) => TokenUpdate::Set(t),
    };

    notify::save(
        &state.pool,
        &state.enc_key,
        user.id,
        body.ntfy_server_url
            .as_deref()
            .or(current.ntfy_server_url.as_deref()),
        body.ntfy_topic.as_deref().or(current.ntfy_topic.as_deref()),
        body.ntfy_priority.unwrap_or(current.ntfy_priority),
        body.notify_on_digest.unwrap_or(current.notify_on_digest),
        body.notify_on_feed_health
            .unwrap_or(current.notify_on_feed_health),
        token,
    )
    .await
    .map_err(AppError::BadRequest)?;

    let updated = notify::load(&state.pool, user.id)
        .await
        .map_err(AppError::Internal)?;
    Ok(Json(updated.into()))
}

#[derive(Serialize)]
struct TestResult {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// `POST /api/notifications/test` - send a test push; report ok/error. Never echoes the token.
async fn test_notifications(user: CurrentUser, State(state): State<AppState>) -> Json<TestResult> {
    match notify::test(&state.pool, &state.http_client, &state.enc_key, user.id).await {
        Ok(()) => Json(TestResult {
            ok: true,
            error: None,
        }),
        Err(e) => Json(TestResult {
            ok: false,
            error: Some(e),
        }),
    }
}

/// Distinguish "field absent" from "field explicitly null" so a client can clear the token by
/// sending `"auth_token": ""` without wiping it on every unrelated PUT.
fn double_option<'de, D>(de: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<String>::deserialize(de)?))
}
