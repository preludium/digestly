//! Digest history + engine control (prompt.md §7, §9.7 Digest tab, §9.8/§9.9, §10).
//!
//! `GET /api/digest` / `GET /api/digest/{id}` are the **current user's** history (per-user
//! scoping). `POST /api/digest/run` and the engine config (`GET/PUT /api/digest/config`) are
//! **admin-only** - enforced server-side via `AdminUser` (§11), runs for all users.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;

use crate::auth::extract::{AdminUser, CurrentUser};
use crate::digest::{self, CategoryFilter, DigestConfig};
use crate::error::{ApiResult, AppError};
use crate::http::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/digest", get(list_digests))
        .route("/digest/config", get(get_config).put(put_config))
        .route("/digest/run", post(run_digest))
        .route("/digest/:id", get(get_digest))
}

// ---------------------------------------------------------------------------
// History (per-user)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct DigestListItem {
    id: i64,
    created_at: String,
    period_start: String,
    period_end: String,
    item_count: i64,
    notified: bool,
    error: Option<String>,
}

/// `GET /api/digest` - the caller's digest history, newest first (§9.8).
async fn list_digests(
    user: CurrentUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<DigestListItem>>> {
    let rows = sqlx::query(
        "SELECT id, created_at, period_start, period_end, item_count, notified, error
         FROM digests WHERE user_id = ? ORDER BY created_at DESC, id DESC",
    )
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(
        rows.iter()
            .map(|r| DigestListItem {
                id: r.get("id"),
                created_at: r.get("created_at"),
                period_start: r.get("period_start"),
                period_end: r.get("period_end"),
                item_count: r.get("item_count"),
                notified: r.get::<i64, _>("notified") != 0,
                error: r.get("error"),
            })
            .collect(),
    ))
}

#[derive(Serialize)]
struct DigestDetail {
    id: i64,
    created_at: String,
    period_start: String,
    period_end: String,
    item_count: i64,
    notified: bool,
    error: Option<String>,
    /// The archived structured payload (categories, sources, notes) - see `digest::build_and_archive`.
    payload: Value,
}

/// `GET /api/digest/{id}` - one archived digest, scoped to the caller (§9.9). 404 if not theirs.
async fn get_digest(
    user: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<DigestDetail>> {
    let row = sqlx::query(
        "SELECT id, created_at, period_start, period_end, item_count, notified, error, payload_json
         FROM digests WHERE id = ? AND user_id = ?",
    )
    .bind(id)
    .bind(user.id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("digest not found".into()))?;

    let payload: Value = row
        .get::<Option<String>, _>("payload_json")
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);

    Ok(Json(DigestDetail {
        id: row.get("id"),
        created_at: row.get("created_at"),
        period_start: row.get("period_start"),
        period_end: row.get("period_end"),
        item_count: row.get("item_count"),
        notified: row.get::<i64, _>("notified") != 0,
        error: row.get("error"),
        payload,
    }))
}

// ---------------------------------------------------------------------------
// Engine config (admin-only)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ConfigDto {
    enabled: bool,
    cron: String,
    lookback_days: i64,
    timezone: String,
    /// `null` = all categories, else the included category names.
    categories: Option<Vec<String>>,
    ai_enabled: bool,
    schedule_preview: String,
}

fn to_dto(c: &DigestConfig) -> ConfigDto {
    ConfigDto {
        enabled: c.enabled,
        cron: c.cron.clone(),
        lookback_days: c.lookback_days,
        timezone: c.timezone.clone(),
        categories: match &c.categories {
            CategoryFilter::All => None,
            CategoryFilter::Names(n) => Some(n.clone()),
        },
        ai_enabled: c.ai_enabled,
        schedule_preview: c.schedule_preview(),
    }
}

async fn get_config(_admin: AdminUser, State(state): State<AppState>) -> Json<ConfigDto> {
    let cfg = DigestConfig::load(&state.pool).await;
    Json(to_dto(&cfg))
}

#[derive(Deserialize)]
struct PutConfig {
    enabled: bool,
    cron: String,
    lookback_days: i64,
    timezone: String,
    categories: Option<Vec<String>>,
    ai_enabled: bool,
}

async fn put_config(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<PutConfig>,
) -> ApiResult<Json<ConfigDto>> {
    // Validate the cron and timezone before persisting so the engine can't be wedged (§9.7).
    if digest::cron::Cron::parse(body.cron.trim()).is_none() {
        return Err(AppError::BadRequest(
            "invalid cron expression (expected 5 fields, e.g. '0 9 * * 1')".into(),
        ));
    }
    if body.timezone.parse::<chrono_tz::Tz>().is_err() {
        return Err(AppError::BadRequest(
            "unknown timezone (use an IANA name like 'Europe/Warsaw')".into(),
        ));
    }

    let cfg = DigestConfig {
        enabled: body.enabled,
        cron: body.cron.trim().to_string(),
        lookback_days: body.lookback_days,
        timezone: body.timezone,
        categories: match body.categories {
            None => CategoryFilter::All,
            Some(n) if n.is_empty() => CategoryFilter::All,
            Some(n) => CategoryFilter::Names(n),
        },
        ai_enabled: body.ai_enabled,
    };
    cfg.save(&state.pool).await.map_err(AppError::Internal)?;
    Ok(Json(to_dto(&cfg)))
}

// ---------------------------------------------------------------------------
// Manual run (admin-only)
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct RunBody {
    /// One-off override for this run only; never persisted (§9.7 manual "longer period" run).
    lookback_days: Option<i64>,
}

/// `POST /api/digest/run` - run the digest for all users now (§7, §10). Admin-only (§11). An
/// optional `lookback_days` in the body overrides the configured window for this run only.
async fn run_digest(
    _admin: AdminUser,
    State(state): State<AppState>,
    body: Option<Json<RunBody>>,
) -> ApiResult<Json<digest::RunSummary>> {
    let lookback_override = body.and_then(|b| b.0.lookback_days);
    let summary = digest::run_all(
        &state.pool,
        &state.http_client,
        &state.enc_key,
        lookback_override,
    )
    .await
    .map_err(AppError::Internal)?;
    Ok(Json(summary))
}
