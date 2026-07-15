//! Admin user-management + global settings (prompt.md §9.13, §10, §11). All endpoints require
//! `role = admin` (via `AdminUser`). Guardrails: the built-in `admin` can't be demoted/disabled/
//! deleted, and the instance always keeps ≥1 enabled admin. Admins never see feed contents.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::auth::extract::AdminUser;
use crate::auth::{Role, ADMIN_USERNAME};
use crate::error::{ApiResult, AppError};
use crate::http::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/admin/users", get(list_users))
        .route(
            "/admin/users/:id",
            axum::routing::patch(update_user).delete(delete_user),
        )
        .route("/admin/settings", get(get_settings).put(put_settings))
        .route("/admin/ingestion", get(get_ingestion).put(put_ingestion))
        .route("/admin/retention/purge", post(purge_now))
}

#[derive(Serialize)]
struct AdminUserDto {
    id: i64,
    username: String,
    role: Role,
    disabled: bool,
    created_at: String,
    last_login_at: Option<String>,
    subscription_count: i64,
}

/// `GET /api/admin/users` - all accounts with subscription counts (account data only, no feeds).
async fn list_users(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<AdminUserDto>>> {
    let rows = sqlx::query(
        "SELECT u.id, COALESCE(u.display_username, u.username) AS username,
                u.role, u.disabled, u.created_at, u.last_login_at,
                (SELECT COUNT(*) FROM subscriptions s WHERE s.user_id = u.id) AS sub_count
         FROM users u
         ORDER BY u.id",
    )
    .fetch_all(&state.pool)
    .await?;

    let users = rows
        .into_iter()
        .map(|r| AdminUserDto {
            id: r.get("id"),
            username: r.get("username"),
            role: Role::from_db(r.get::<String, _>("role").as_str()),
            disabled: r.get::<i64, _>("disabled") != 0,
            created_at: r.get("created_at"),
            last_login_at: r.get("last_login_at"),
            subscription_count: r.get("sub_count"),
        })
        .collect();
    Ok(Json(users))
}

#[derive(Deserialize)]
struct UpdateUser {
    role: Option<Role>,
    disabled: Option<bool>,
}

/// `PATCH /api/admin/users/{id}` - change role and/or enabled state, with guardrails.
async fn update_user(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateUser>,
) -> ApiResult<Json<serde_json::Value>> {
    let target = fetch_target(&state.pool, id).await?;

    // The built-in admin is immutable.
    if target.username == ADMIN_USERNAME
        && (matches!(body.role, Some(Role::User)) || body.disabled == Some(true))
    {
        return Err(AppError::Forbidden);
    }

    // Would this change remove the last enabled admin?
    let demoting = matches!(body.role, Some(Role::User)) && target.role == Role::Admin;
    let disabling = body.disabled == Some(true) && target.role == Role::Admin && !target.disabled;
    if (demoting || disabling) && enabled_admin_count(&state.pool).await? <= 1 {
        return Err(AppError::Conflict("cannot remove the last admin".into()));
    }

    if let Some(role) = body.role {
        sqlx::query("UPDATE users SET role = ? WHERE id = ?")
            .bind(role.as_str())
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(disabled) = body.disabled {
        sqlx::query("UPDATE users SET disabled = ? WHERE id = ?")
            .bind(disabled as i64)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `DELETE /api/admin/users/{id}` - delete an account (cascades per-user data), with guardrails.
async fn delete_user(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let target = fetch_target(&state.pool, id).await?;
    if target.username == ADMIN_USERNAME {
        return Err(AppError::Forbidden);
    }
    if target.role == Role::Admin
        && !target.disabled
        && enabled_admin_count(&state.pool).await? <= 1
    {
        return Err(AppError::Conflict("cannot remove the last admin".into()));
    }
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Serialize, Deserialize)]
struct SettingsDto {
    allow_registration: bool,
}

/// `GET /api/admin/settings` - global admin settings.
async fn get_settings(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> ApiResult<Json<SettingsDto>> {
    Ok(Json(SettingsDto {
        allow_registration: crate::routes::auth::allow_registration(&state.pool).await?,
    }))
}

/// `PUT /api/admin/settings` - update global admin settings.
async fn put_settings(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<SettingsDto>,
) -> ApiResult<Json<SettingsDto>> {
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES ('allow_registration', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(if body.allow_registration {
        "true"
    } else {
        "false"
    })
    .execute(&state.pool)
    .await?;
    Ok(Json(body))
}

// ---------------------------------------------------------------------------
// Ingestion + retention settings (admin-only, prompt.md §8)
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct IngestionDto {
    concurrency: i64,
    per_host_delay_ms: i64,
    timeout_secs: i64,
    default_interval_secs: i64,
    allow_private: bool,
    /// Items published earlier than N days ago are skipped at ingest time (0 = no cutoff).
    #[serde(default)]
    max_item_age_days: i64,
    /// Purge non-starred items older than N days (0 = keep forever).
    retention_max_age_days: i64,
    /// Keep at most M newest non-starred items per feed (0 = unlimited).
    retention_max_per_feed: i64,
}

/// `GET /api/admin/ingestion` - the effective ingestion + retention tunables.
async fn get_ingestion(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> ApiResult<Json<IngestionDto>> {
    let cfg = crate::ingest::settings::IngestSettings::load(&state.pool).await;
    let ret = crate::maintenance::RetentionPolicy::load(&state.pool).await;
    Ok(Json(IngestionDto {
        concurrency: cfg.concurrency as i64,
        per_host_delay_ms: cfg.per_host_delay_ms as i64,
        timeout_secs: cfg.timeout_secs as i64,
        default_interval_secs: cfg.default_interval_secs,
        allow_private: cfg.allow_private,
        max_item_age_days: cfg.max_item_age_days,
        retention_max_age_days: ret.max_age_days,
        retention_max_per_feed: ret.max_per_feed,
    }))
}

/// `PUT /api/admin/ingestion` - persist the tunables (clamped to sane ranges).
async fn put_ingestion(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(b): Json<IngestionDto>,
) -> ApiResult<Json<IngestionDto>> {
    set_app(
        &state.pool,
        "ingest.concurrency",
        &b.concurrency.clamp(1, 64).to_string(),
    )
    .await?;
    set_app(
        &state.pool,
        "ingest.per_host_delay_ms",
        &b.per_host_delay_ms.clamp(0, 60_000).to_string(),
    )
    .await?;
    set_app(
        &state.pool,
        "ingest.timeout_secs",
        &b.timeout_secs.clamp(1, 120).to_string(),
    )
    .await?;
    set_app(
        &state.pool,
        "ingest.default_interval_secs",
        &b.default_interval_secs.clamp(60, 86_400).to_string(),
    )
    .await?;
    set_app(
        &state.pool,
        "ingest.allow_private",
        if b.allow_private { "true" } else { "false" },
    )
    .await?;
    set_app(
        &state.pool,
        "ingest.max_item_age_days",
        &b.max_item_age_days.max(0).to_string(),
    )
    .await?;
    set_app(
        &state.pool,
        "retention.max_age_days",
        &b.retention_max_age_days.max(0).to_string(),
    )
    .await?;
    set_app(
        &state.pool,
        "retention.max_per_feed",
        &b.retention_max_per_feed.max(0).to_string(),
    )
    .await?;
    get_ingestion(_admin, State(state)).await
}

#[derive(Serialize)]
struct PurgeResult {
    removed: u64,
}

/// `POST /api/admin/retention/purge` - apply the saved retention policy right now instead of
/// waiting for the periodic 6h maintenance task (§8). Uses the exact same purge logic; starred
/// items are always kept.
async fn purge_now(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> ApiResult<Json<PurgeResult>> {
    let removed = crate::maintenance::purge(&state.pool)
        .await
        .map_err(AppError::Internal)?;
    Ok(Json(PurgeResult { removed }))
}

async fn set_app(pool: &SqlitePool, key: &str, value: &str) -> ApiResult<()> {
    sqlx::query("INSERT INTO app_settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(key)
        .bind(value)
        .execute(pool)
        .await?;
    Ok(())
}

struct TargetUser {
    username: String,
    role: Role,
    disabled: bool,
}

async fn fetch_target(pool: &SqlitePool, id: i64) -> ApiResult<TargetUser> {
    // Return the CANONICAL (normalized) username, NOT the display value: this row feeds
    // `target.username == ADMIN_USERNAME` guards (built-in-admin protection at `update_user` and
    // `delete_user`), which are case-sensitive `==` against the stored normalized form.
    let row = sqlx::query("SELECT username, role, disabled FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".into()))?;
    Ok(TargetUser {
        username: row.get("username"),
        role: Role::from_db(row.get::<String, _>("role").as_str()),
        disabled: row.get::<i64, _>("disabled") != 0,
    })
}

async fn enabled_admin_count(pool: &SqlitePool) -> ApiResult<i64> {
    let count: i64 =
        sqlx::query("SELECT COUNT(*) AS c FROM users WHERE role = 'admin' AND disabled = 0")
            .fetch_one(pool)
            .await?
            .get("c");
    Ok(count)
}
