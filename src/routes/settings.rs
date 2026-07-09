//! Per-user preferences (prompt.md §8, §9.7 General tab, §10). Stored in the per-user `settings`
//! table (key/value), scoped to the session user. These are *preferences only* — engine config
//! lives in `app_settings` (admin-only). Also carries the one-shot `onboarded` flag (§9.11).

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::auth::extract::CurrentUser;
use crate::error::{ApiResult, AppError};
use crate::http::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/settings", get(get_settings).put(put_settings))
        .route("/onboarding/starter-feeds", axum::routing::post(subscribe_starter_feeds))
}

/// `POST /api/onboarding/starter-feeds` — opt-in subscribe to the §3 starter set during onboarding
/// (§9.11), mapping each to the user's seeded category by name. Idempotent; returns how many were
/// added. Never force-run — the client calls this only if the user chooses the starter set.
async fn subscribe_starter_feeds(user: CurrentUser, State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    use crate::ingest::settings::IngestSettings;
    use crate::ingest::FeedKind;

    let cfg = IngestSettings::load(&state.pool).await;
    let mut added = 0usize;
    for (feed_url, kind, category) in crate::seed::STARTER_FEEDS {
        // Create category on demand — only "Other" is seeded now (§TODO-9).
        sqlx::query("INSERT OR IGNORE INTO categories (user_id, name, position) VALUES (?, ?, (SELECT COALESCE(MAX(position), 0) + 1 FROM categories WHERE user_id = ?))")
            .bind(user.id)
            .bind(category)
            .bind(user.id)
            .execute(&state.pool)
            .await?;
        let cat_id: i64 = sqlx::query("SELECT id FROM categories WHERE user_id = ? AND name = ?")
            .bind(user.id)
            .bind(category)
            .fetch_one(&state.pool)
            .await?
            .get("id");
        if crate::routes::feeds::subscribe_url(&state.pool, &cfg, user.id, feed_url, FeedKind::from_db(kind), cat_id, None, true).await? {
            added += 1;
        }
    }
    if added > 0 {
        state.ingest_trigger.notify_one();
    }
    Ok(Json(serde_json::json!({ "added": added })))
}

/// The user's resolved preferences (defaults filled in for any unset key).
#[derive(Serialize)]
struct SettingsDto {
    sort: String,
    content_view: String,
    page_size: i64,
    timezone: String,
    density: String,
    auto_mark_read: bool,
    theme: String,
    onboarded: bool,
}

impl Default for SettingsDto {
    fn default() -> Self {
        Self {
            sort: "new".into(),
            content_view: "all".into(),
            page_size: 50,
            timezone: "UTC".into(),
            density: "normal".into(),
            auto_mark_read: false,
            theme: "dark".into(),
            onboarded: false,
        }
    }
}

async fn get_settings(user: CurrentUser, State(state): State<AppState>) -> ApiResult<Json<SettingsDto>> {
    Ok(Json(load(&state.pool, user.id).await?))
}

/// PUT accepts any subset; unspecified fields keep their current value.
#[derive(Deserialize)]
struct PutSettings {
    sort: Option<String>,
    content_view: Option<String>,
    page_size: Option<i64>,
    timezone: Option<String>,
    density: Option<String>,
    auto_mark_read: Option<bool>,
    theme: Option<String>,
    onboarded: Option<bool>,
}

async fn put_settings(
    user: CurrentUser,
    State(state): State<AppState>,
    Json(body): Json<PutSettings>,
) -> ApiResult<Json<SettingsDto>> {
    if let Some(s) = &body.sort {
        if !["new", "old", "quick", "top", "discussed", "unread"].contains(&s.as_str()) {
            return Err(AppError::BadRequest("invalid sort".into()));
        }
        set(&state.pool, user.id, "sort", s).await?;
    }
    if let Some(v) = &body.content_view {
        if !["all", "reading", "video"].contains(&v.as_str()) {
            return Err(AppError::BadRequest("invalid content view".into()));
        }
        set(&state.pool, user.id, "content_view", v).await?;
    }
    if let Some(p) = body.page_size {
        set(&state.pool, user.id, "page_size", &p.clamp(1, 100).to_string()).await?;
    }
    if let Some(tz) = &body.timezone {
        if tz.parse::<chrono_tz::Tz>().is_err() {
            return Err(AppError::BadRequest("unknown timezone (use an IANA name like 'Europe/Warsaw')".into()));
        }
        set(&state.pool, user.id, "timezone", tz).await?;
    }
    if let Some(d) = &body.density {
        if !["normal", "compact"].contains(&d.as_str()) {
            return Err(AppError::BadRequest("invalid density".into()));
        }
        set(&state.pool, user.id, "density", d).await?;
    }
    if let Some(b) = body.auto_mark_read {
        set(&state.pool, user.id, "auto_mark_read", bool_str(b)).await?;
    }
    if let Some(t) = &body.theme {
        if !["light", "dark"].contains(&t.as_str()) {
            return Err(AppError::BadRequest("invalid theme".into()));
        }
        set(&state.pool, user.id, "theme", t).await?;
    }
    if let Some(b) = body.onboarded {
        set(&state.pool, user.id, "onboarded", bool_str(b)).await?;
    }
    Ok(Json(load(&state.pool, user.id).await?))
}

// ---------------------------------------------------------------------------
// Helpers (also used by onboarding/opml)
// ---------------------------------------------------------------------------

async fn load(pool: &SqlitePool, user_id: i64) -> ApiResult<SettingsDto> {
    let rows = sqlx::query("SELECT key, value FROM settings WHERE user_id = ?")
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    let mut d = SettingsDto::default();
    for r in &rows {
        let k: String = r.get("key");
        let v: String = r.get("value");
        match k.as_str() {
            "sort" => d.sort = v,
            "content_view" => d.content_view = v,
            "page_size" => d.page_size = v.parse().unwrap_or(d.page_size),
            "timezone" => d.timezone = v,
            "density" => d.density = v,
            "auto_mark_read" => d.auto_mark_read = v == "true" || v == "1",
            "theme" => d.theme = v,
            "onboarded" => d.onboarded = v == "true" || v == "1",
            _ => {}
        }
    }
    Ok(d)
}

async fn set(pool: &SqlitePool, user_id: i64, key: &str, value: &str) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO settings (user_id, key, value) VALUES (?, ?, ?)
         ON CONFLICT(user_id, key) DO UPDATE SET value = excluded.value",
    )
    .bind(user_id)
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

fn bool_str(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}
