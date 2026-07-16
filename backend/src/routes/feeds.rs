//! Feeds = a user's subscriptions over the global feed catalog (prompt.md §9.3–9.6, §10, §11).
//! Everything is scoped to the session user; the shared `feeds`/`items` rows are polled once.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::auth::extract::CurrentUser;
use crate::error::{ApiResult, AppError};
use crate::http::AppState;
use crate::ingest::discover::{self, Candidate};
use crate::ingest::settings::IngestSettings;
use crate::ingest::{url_util, FeedKind};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/feeds", get(list_feeds).post(subscribe))
        .route("/feeds/discover", post(discover_feeds))
        .route("/feeds/health", get(health))
        .route("/feeds/refresh-all", post(refresh_all))
        .route(
            "/feeds/:id",
            axum::routing::patch(update_feed).delete(unsubscribe),
        )
        .route("/feeds/:id/refresh", post(refresh_feed))
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct DiscoverBody {
    input: String,
}

#[derive(Serialize)]
struct DiscoverCandidate {
    #[serde(flatten)]
    candidate: Candidate,
    already_subscribed: bool,
}

/// `POST /api/feeds/discover` - resolve arbitrary input to feed candidates (§9.3).
async fn discover_feeds(
    user: CurrentUser,
    State(state): State<AppState>,
    Json(body): Json<DiscoverBody>,
) -> ApiResult<Json<Vec<DiscoverCandidate>>> {
    let cfg = IngestSettings::load(&state.pool).await;
    let candidates = discover::discover(&state.http_client, &cfg, &body.input)
        .await
        .map_err(AppError::Internal)?;

    let mut out = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let already_subscribed = existing_subscription(&state.pool, user.id, &candidate.feed_url)
            .await?
            .is_some();
        out.push(DiscoverCandidate {
            candidate,
            already_subscribed,
        });
    }
    Ok(Json(out))
}

// ---------------------------------------------------------------------------
// Subscribe / list / edit / unsubscribe
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SubscribeBody {
    feed_url: String,
    kind: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    site_url: Option<String>,
    /// REQUIRED (prompt.md §9.3, §11): every subscription belongs to exactly one category.
    category_id: i64,
    #[serde(default)]
    content_type: Option<String>,
    #[serde(default)]
    min_score: Option<i64>,
    #[serde(default)]
    full_text_extract: Option<bool>,
    #[serde(default)]
    title_override: Option<String>,
    #[serde(default)]
    fetch_interval_secs: Option<i64>,
}

/// `POST /api/feeds` - subscribe the user to a feed (creating the global feed row if new).
async fn subscribe(
    user: CurrentUser,
    State(state): State<AppState>,
    Json(body): Json<SubscribeBody>,
) -> ApiResult<Json<FeedDto>> {
    require_category(&state.pool, user.id, body.category_id).await?;

    let kind = FeedKind::from_db(&body.kind);
    let feed_url = url_util::normalize_url(&body.feed_url)
        .ok_or_else(|| AppError::BadRequest("invalid feed URL".into()))?;

    // Dedupe against an already-subscribed feed (http/https/trailing-slash variants).
    if existing_subscription(&state.pool, user.id, &feed_url)
        .await?
        .is_some()
    {
        return Err(AppError::Conflict(
            "you are already subscribed to this feed".into(),
        ));
    }

    let cfg = IngestSettings::load(&state.pool).await;
    let feed_id = upsert_feed(
        &state.pool,
        &feed_url,
        kind,
        body.site_url.as_deref(),
        body.title.as_deref(),
        &cfg,
        true,
    )
    .await?;

    let content_type = body
        .content_type
        .filter(|c| c == "reading" || c == "video")
        .unwrap_or_else(|| kind.default_content_type().to_string());

    let sub_id: i64 = sqlx::query(
        "INSERT INTO subscriptions
            (user_id, feed_id, category_id, content_type, min_score, full_text_extract, title_override)
         VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(user.id)
    .bind(feed_id)
    .bind(body.category_id)
    .bind(&content_type)
    .bind(body.min_score.unwrap_or(0).max(0))
    .bind(body.full_text_extract.unwrap_or(false) as i64)
    .bind(body.title_override.filter(|s| !s.trim().is_empty()))
    .fetch_one(&state.pool)
    .await?
    .get("id");

    if let Some(interval) = body.fetch_interval_secs.filter(|i| *i >= 60) {
        sqlx::query("UPDATE feeds SET fetch_interval_secs = ? WHERE id = ?")
            .bind(interval)
            .bind(feed_id)
            .execute(&state.pool)
            .await?;
    }

    // Poll promptly and wake the scheduler (§4 - new subscription makes the feed eligible).
    mark_due(&state.pool, feed_id).await?;
    state.ingest_trigger.notify_one();

    let dto = fetch_feed_dto(&state.pool, user.id, sub_id).await?;
    Ok(Json(dto))
}

#[derive(Deserialize)]
struct UpdateFeedBody {
    category_id: Option<i64>,
    content_type: Option<String>,
    min_score: Option<i64>,
    full_text_extract: Option<bool>,
    disabled: Option<bool>,
    title_override: Option<String>,
    fetch_interval_secs: Option<i64>,
}

/// `PATCH /api/feeds/{id}` - edit the user's subscription (§9.4). Category stays required.
async fn update_feed(
    user: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateFeedBody>,
) -> ApiResult<Json<FeedDto>> {
    let feed_id = owned_subscription_feed(&state.pool, user.id, id).await?;

    if let Some(cat) = body.category_id {
        require_category(&state.pool, user.id, cat).await?;
        sqlx::query("UPDATE subscriptions SET category_id = ? WHERE id = ? AND user_id = ?")
            .bind(cat)
            .bind(id)
            .bind(user.id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(ct) = body.content_type.filter(|c| c == "reading" || c == "video") {
        sqlx::query("UPDATE subscriptions SET content_type = ? WHERE id = ? AND user_id = ?")
            .bind(ct)
            .bind(id)
            .bind(user.id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(ms) = body.min_score {
        sqlx::query("UPDATE subscriptions SET min_score = ? WHERE id = ? AND user_id = ?")
            .bind(ms.max(0))
            .bind(id)
            .bind(user.id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(ft) = body.full_text_extract {
        sqlx::query("UPDATE subscriptions SET full_text_extract = ? WHERE id = ? AND user_id = ?")
            .bind(ft as i64)
            .bind(id)
            .bind(user.id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(dis) = body.disabled {
        sqlx::query("UPDATE subscriptions SET disabled = ? WHERE id = ? AND user_id = ?")
            .bind(dis as i64)
            .bind(id)
            .bind(user.id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(t) = body.title_override {
        let val = t.trim().to_string();
        sqlx::query("UPDATE subscriptions SET title_override = ? WHERE id = ? AND user_id = ?")
            .bind(if val.is_empty() { None } else { Some(val) })
            .bind(id)
            .bind(user.id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(interval) = body.fetch_interval_secs.filter(|i| *i >= 60) {
        sqlx::query("UPDATE feeds SET fetch_interval_secs = ? WHERE id = ?")
            .bind(interval)
            .bind(feed_id)
            .execute(&state.pool)
            .await?;
    }

    Ok(Json(fetch_feed_dto(&state.pool, user.id, id).await?))
}

/// `DELETE /api/feeds/{id}` - unsubscribe. The global feed keeps its items; it just stops being
/// polled once it has no active subscriptions (§4, §11).
async fn unsubscribe(
    user: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    owned_subscription_feed(&state.pool, user.id, id).await?;
    sqlx::query("DELETE FROM subscriptions WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user.id)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `POST /api/feeds/{id}/refresh` - poll now; also re-enables + clears backoff so this doubles as
/// the health page's "retry now / re-enable" action (§9.6).
async fn refresh_feed(
    user: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let feed_id = owned_subscription_feed(&state.pool, user.id, id).await?;
    sqlx::query(
        "UPDATE feeds SET disabled = 0, failure_count = 0, last_error = NULL,
                          next_fetch_at = datetime('now') WHERE id = ?",
    )
    .bind(feed_id)
    .execute(&state.pool)
    .await?;
    state.ingest_trigger.notify_one();
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `POST /api/feeds/refresh-all` - "Ingest now": re-poll every feed the user is subscribed to, in
/// one DB write and one scheduler wakeup (§9.0). Scoped to the caller's own subscriptions only -
/// `feeds` is shared across users.
///
/// The poll itself happens later, in the background scheduler, so this returns before a single
/// feed has been fetched. The returned `run_id` is what the client watches on the SSE stream to
/// learn when its ingestion actually finished (see `crate::events`).
async fn refresh_all(
    user: CurrentUser,
    State(state): State<AppState>,
) -> ApiResult<Json<serde_json::Value>> {
    // Only feeds the scheduler will actually poll (`select_due` skips disabled subscriptions) -
    // a feed in the run that never gets polled would strand the run until its TTL.
    let feed_ids: Vec<i64> =
        sqlx::query_scalar("SELECT feed_id FROM subscriptions WHERE user_id = ? AND disabled = 0")
            .bind(user.id)
            .fetch_all(&state.pool)
            .await?;

    if feed_ids.is_empty() {
        return Ok(Json(
            serde_json::json!({ "ok": true, "run_id": null, "feeds": 0 }),
        ));
    }

    // Open the run before waking the scheduler: a feed that finished polling before its run
    // existed would never be counted, and the run would hang until the sweeper.
    let run_id = state
        .events
        .open_run(user.id, feed_ids.clone())
        .map_err(|secs| {
            AppError::TooManyRequests(format!("ingest is on cooldown - try again in {secs}s"))
        })?;

    sqlx::query(
        "UPDATE feeds SET disabled = 0, failure_count = 0, last_error = NULL, next_fetch_at = datetime('now')
         WHERE id IN (SELECT feed_id FROM subscriptions WHERE user_id = ? AND disabled = 0)",
    )
    .bind(user.id)
    .execute(&state.pool)
    .await?;
    state.ingest_trigger.notify_one();

    Ok(Json(
        serde_json::json!({ "ok": true, "run_id": run_id, "feeds": feed_ids.len() }),
    ))
}

// ---------------------------------------------------------------------------
// List + health
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct FeedDto {
    id: i64,
    feed_id: i64,
    feed_url: String,
    title: String,
    kind: String,
    site_url: Option<String>,
    icon_url: Option<String>,
    category_id: i64,
    category_name: String,
    content_type: String,
    min_score: i64,
    full_text_extract: bool,
    fetch_interval_secs: i64,
    disabled: bool,
    item_count: i64,
    last_fetch_at: Option<String>,
    last_error: Option<String>,
    failure_count: i64,
    feed_disabled: bool,
}

const FEED_SELECT: &str = "
    SELECT s.id AS id, s.feed_id AS feed_id, f.feed_url AS feed_url,
           COALESCE(NULLIF(s.title_override, ''), NULLIF(f.title, ''), f.feed_url) AS title,
           f.kind AS kind, f.site_url AS site_url, f.icon_url AS icon_url,
           s.category_id AS category_id, c.name AS category_name,
           s.content_type AS content_type, s.min_score AS min_score,
           s.full_text_extract AS full_text_extract, f.fetch_interval_secs AS fetch_interval_secs,
           s.disabled AS disabled,
           (SELECT COUNT(*) FROM items i WHERE i.feed_id = f.id) AS item_count,
           f.last_fetch_at AS last_fetch_at, f.last_error AS last_error,
           f.failure_count AS failure_count, f.disabled AS feed_disabled,
           f.next_fetch_at AS next_fetch_at
    FROM subscriptions s
    JOIN feeds f ON f.id = s.feed_id
    JOIN categories c ON c.id = s.category_id
    WHERE s.user_id = ?";

fn row_to_dto(r: &sqlx::sqlite::SqliteRow) -> FeedDto {
    FeedDto {
        id: r.get("id"),
        feed_id: r.get("feed_id"),
        feed_url: r.get("feed_url"),
        title: r.get("title"),
        kind: r.get("kind"),
        site_url: r.get("site_url"),
        icon_url: r.get("icon_url"),
        category_id: r.get("category_id"),
        category_name: r.get("category_name"),
        content_type: r.get("content_type"),
        min_score: r.get("min_score"),
        full_text_extract: r.get::<i64, _>("full_text_extract") != 0,
        fetch_interval_secs: r.get("fetch_interval_secs"),
        disabled: r.get::<i64, _>("disabled") != 0,
        item_count: r.get("item_count"),
        last_fetch_at: r.get("last_fetch_at"),
        last_error: r.get("last_error"),
        failure_count: r.get("failure_count"),
        feed_disabled: r.get::<i64, _>("feed_disabled") != 0,
    }
}

/// `GET /api/feeds` - the user's subscriptions (§9.5).
async fn list_feeds(
    user: CurrentUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<FeedDto>>> {
    let rows = sqlx::query(&format!("{FEED_SELECT} ORDER BY c.position, title"))
        .bind(user.id)
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(rows.iter().map(row_to_dto).collect()))
}

#[derive(Serialize)]
struct HealthDto {
    id: i64,
    feed_id: i64,
    title: String,
    feed_url: String,
    kind: String,
    status: String, // ok | failing | disabled
    last_fetch_at: Option<String>,
    next_fetch_at: Option<String>,
    failure_count: i64,
    last_error: Option<String>,
}

/// `GET /api/feeds/health` - per-user feed diagnostics (§9.6). Failing/disabled feeds are surfaced
/// here, never silently dropped.
async fn health(
    user: CurrentUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<HealthDto>>> {
    let rows = sqlx::query(&format!(
        "{FEED_SELECT} ORDER BY f.disabled DESC, f.failure_count DESC, title"
    ))
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;

    let out = rows
        .iter()
        .map(|r| {
            let feed_disabled = r.get::<i64, _>("feed_disabled") != 0;
            let failure_count: i64 = r.get("failure_count");
            let status = if feed_disabled {
                "disabled"
            } else if failure_count > 0 {
                "failing"
            } else {
                "ok"
            };
            HealthDto {
                id: r.get("id"),
                feed_id: r.get("feed_id"),
                title: r.get("title"),
                feed_url: r.get("feed_url"),
                kind: r.get("kind"),
                status: status.to_string(),
                last_fetch_at: r.get("last_fetch_at"),
                next_fetch_at: r.get("next_fetch_at"),
                failure_count,
                last_error: r.get("last_error"),
            }
        })
        .collect();
    Ok(Json(out))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Whether the user is already subscribed to `feed_url` (any scheme variant). Used by OPML preview.
pub(crate) async fn is_subscribed(
    pool: &SqlitePool,
    user_id: i64,
    feed_url: &str,
) -> ApiResult<bool> {
    match url_util::normalize_url(feed_url) {
        Some(u) => Ok(existing_subscription(pool, user_id, &u).await?.is_some()),
        None => Ok(false),
    }
}

/// Subscribe `user_id` to `feed_url` under `category_id`, creating the shared feed row if new.
/// Idempotent: returns `false` (skipped) if the user is already subscribed. When `poll_immediately`
/// is true, marks the feed due so the scheduler picks it up right away; callers do a single
/// `ingest_trigger.notify_one()` after a batch. Shared by OPML import (§9.5), onboarding starter
/// feeds (§9.11, both `poll_immediately: true`), and OAuth sync (§3, `poll_immediately: false` -
/// creates the subscription without an immediate backlog poll).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn subscribe_url(
    pool: &SqlitePool,
    cfg: &IngestSettings,
    user_id: i64,
    feed_url: &str,
    kind: FeedKind,
    category_id: i64,
    title: Option<&str>,
    poll_immediately: bool,
) -> ApiResult<bool> {
    let feed_url = match url_util::normalize_url(feed_url) {
        Some(u) => u,
        None => return Ok(false),
    };
    if existing_subscription(pool, user_id, &feed_url)
        .await?
        .is_some()
    {
        return Ok(false);
    }
    let feed_id = upsert_feed(pool, &feed_url, kind, None, title, cfg, poll_immediately).await?;
    sqlx::query(
        "INSERT INTO subscriptions (user_id, feed_id, category_id, content_type)
         VALUES (?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(feed_id)
    .bind(category_id)
    .bind(kind.default_content_type())
    .execute(pool)
    .await?;
    if poll_immediately {
        mark_due(pool, feed_id).await?;
    }
    Ok(true)
}

/// Find-or-create the global feed for a normalized URL (dedupe across http/https variants).
/// `poll_immediately` controls whether a brand-new feed row is due right now (manual add-feed,
/// OPML import) or only after one normal interval (OAuth sync - §3 fix: sync should create the
/// subscription without dumping the channel's historical backlog into the dashboard).
async fn upsert_feed(
    pool: &SqlitePool,
    feed_url: &str,
    kind: FeedKind,
    site_url: Option<&str>,
    title: Option<&str>,
    cfg: &IngestSettings,
    poll_immediately: bool,
) -> ApiResult<i64> {
    for variant in url_util::scheme_variants(feed_url) {
        if let Some(row) = sqlx::query("SELECT id FROM feeds WHERE feed_url = ?")
            .bind(&variant)
            .fetch_optional(pool)
            .await?
        {
            return Ok(row.get("id"));
        }
    }
    let next_fetch_at = if poll_immediately {
        chrono::Utc::now()
    } else {
        default_next_fetch_at(pool, cfg).await
    };
    let id: i64 = sqlx::query(
        "INSERT INTO feeds (feed_url, site_url, title, kind, fetch_interval_secs, next_fetch_at)
         VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(feed_url)
    .bind(site_url)
    .bind(title.filter(|s| !s.trim().is_empty()))
    .bind(kind.as_str())
    .bind(cfg.default_interval_secs)
    .bind(next_fetch_at.format("%Y-%m-%d %H:%M:%S").to_string())
    .fetch_one(pool)
    .await?
    .get("id");
    Ok(id)
}

/// First-poll time for a feed created without an immediate fetch (OAuth sync - keeps the
/// no-backlog-dump behavior). Anchored to `digest::PREFETCH_BUFFER_SECS` before the digest's next
/// scheduled run so it's fresh in time for that run, falling back to the plain default interval
/// if the digest is disabled, its cron is unparseable, or the buffer would land in the past
/// (would otherwise dump the channel's backlog immediately, defeating the point of this path).
async fn default_next_fetch_at(
    pool: &SqlitePool,
    cfg: &IngestSettings,
) -> chrono::DateTime<chrono::Utc> {
    let now = chrono::Utc::now();
    let digest_cfg = crate::digest::DigestConfig::load(pool).await;
    if digest_cfg.enabled {
        if let Some(next_run) = digest_cfg.next_run_at(now) {
            let anchor = next_run - chrono::Duration::seconds(crate::digest::PREFETCH_BUFFER_SECS);
            if anchor > now {
                return anchor;
            }
        }
    }
    now + chrono::Duration::seconds(cfg.default_interval_secs)
}

/// Return the feed_id of a subscription owned by this user, or 404.
async fn owned_subscription_feed(pool: &SqlitePool, user_id: i64, sub_id: i64) -> ApiResult<i64> {
    sqlx::query("SELECT feed_id FROM subscriptions WHERE id = ? AND user_id = ?")
        .bind(sub_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .map(|r| r.get("feed_id"))
        .ok_or_else(|| AppError::NotFound("feed not found".into()))
}

/// The user's subscription for a feed URL (any scheme variant), if any.
async fn existing_subscription(
    pool: &SqlitePool,
    user_id: i64,
    feed_url: &str,
) -> ApiResult<Option<i64>> {
    for variant in url_util::scheme_variants(feed_url) {
        if let Some(row) = sqlx::query(
            "SELECT s.id FROM subscriptions s JOIN feeds f ON f.id = s.feed_id
             WHERE s.user_id = ? AND f.feed_url = ?",
        )
        .bind(user_id)
        .bind(&variant)
        .fetch_optional(pool)
        .await?
        {
            return Ok(Some(row.get("id")));
        }
    }
    Ok(None)
}

async fn require_category(pool: &SqlitePool, user_id: i64, category_id: i64) -> ApiResult<()> {
    let ok = sqlx::query("SELECT 1 FROM categories WHERE id = ? AND user_id = ?")
        .bind(category_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .is_some();
    if ok {
        Ok(())
    } else {
        Err(AppError::BadRequest("a valid category is required".into()))
    }
}

async fn mark_due(pool: &SqlitePool, feed_id: i64) -> ApiResult<()> {
    sqlx::query("UPDATE feeds SET next_fetch_at = datetime('now') WHERE id = ?")
        .bind(feed_id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn fetch_feed_dto(pool: &SqlitePool, user_id: i64, sub_id: i64) -> ApiResult<FeedDto> {
    let row = sqlx::query(&format!("{FEED_SELECT} AND s.id = ?"))
        .bind(user_id)
        .bind(sub_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("feed not found".into()))?;
    Ok(row_to_dto(&row))
}
