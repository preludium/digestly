//! Items API - the core reader (prompt.md §9.1, §9.1a, §9.2, §10, §11).
//!
//! Items and feeds are shared/global; read/star state and `min_score` are per-user and applied at
//! query time. Every query joins the caller's `subscriptions` (from the session, never a client
//! id) so a user only ever sees items from feeds they subscribe to, with their own state.

use std::str::FromStr;

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};

use crate::auth::extract::CurrentUser;
use crate::error::{ApiResult, AppError};
use crate::http::AppState;
use crate::query::{fts_query, parse_tz, sort_clause, when_range};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/items", get(list_items))
        .route("/items/:id", get(get_item))
        .route("/items/:id/read", post(set_read))
        .route("/items/:id/star", post(set_star))
        .route("/items/:id/summarize", post(summarize))
        .route("/categories/counts", get(category_counts))
}

const MAX_PAGE_SIZE: i64 = 100;
const DEFAULT_PAGE_SIZE: i64 = 50;

// ---------------------------------------------------------------------------
// Shared scoping / filter clause
// ---------------------------------------------------------------------------

/// Resolved, owned filter values pushed as binds. Category/feed/q are only used by the list
/// endpoint; `category_counts` leaves them `None` so chip counts reflect the *other* facets.
#[derive(Default)]
struct Filters {
    content_type: Option<String>,
    status: Option<String>,
    category: Option<i64>,
    feed: Option<i64>,
    when_start: Option<String>,
    when_end: Option<String>,
    q: Option<String>,
}

/// Append the shared `FROM … JOIN … WHERE …` (per-user scope + facets) to a builder, so the count
/// and page queries stay in lockstep. Starts with a leading space; the caller supplies the SELECT.
fn push_scope(qb: &mut QueryBuilder<'_, Sqlite>, user_id: i64, f: &Filters) {
    qb.push(" FROM items i JOIN subscriptions s ON s.feed_id = i.feed_id AND s.user_id = ");
    qb.push_bind(user_id);
    qb.push(
        " JOIN feeds fe ON fe.id = i.feed_id \
          JOIN categories c ON c.id = s.category_id \
          LEFT JOIN item_states st ON st.item_id = i.id AND st.user_id = ",
    );
    qb.push_bind(user_id);
    // Paused subscriptions drop out of the reader; Reddit min_score applied to shared items. A
    // NULL score (e.g. the Reddit JSON endpoint got blocked and ingestion fell back to plain
    // .rss, which carries no vote data) must not bypass a real threshold - unknown is treated as
    // "too low", not "let it through". min_score<=0 ("off") still shows everything.
    qb.push(" WHERE s.disabled = 0 AND (s.min_score <= 0 OR (i.score IS NOT NULL AND i.score >= s.min_score))");

    if let Some(ct) = &f.content_type {
        qb.push(" AND s.content_type = ");
        qb.push_bind(ct.clone());
    }
    match f.status.as_deref() {
        Some("unread") => {
            qb.push(" AND COALESCE(st.is_read, 0) = 0");
        }
        Some("starred") => {
            qb.push(" AND COALESCE(st.is_starred, 0) = 1");
        }
        _ => {}
    }
    if let Some(cat) = f.category {
        qb.push(" AND s.category_id = ");
        qb.push_bind(cat);
    }
    if let Some(feed) = f.feed {
        qb.push(" AND i.feed_id = ");
        qb.push_bind(feed);
    }
    if let Some(start) = &f.when_start {
        qb.push(" AND i.published_at >= ");
        qb.push_bind(start.clone());
    }
    if let Some(end) = &f.when_end {
        qb.push(" AND i.published_at < ");
        qb.push_bind(end.clone());
    }
    if let Some(q) = &f.q {
        qb.push(" AND i.id IN (SELECT rowid FROM items_fts WHERE items_fts MATCH ");
        qb.push_bind(q.clone());
        qb.push(")");
    }
}

// ---------------------------------------------------------------------------
// GET /api/items
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ItemQuery {
    r#type: Option<String>,
    status: Option<String>,
    category: Option<String>,
    feed: Option<i64>,
    when: Option<String>,
    q: Option<String>,
    sort: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

#[derive(Serialize)]
struct ItemDto {
    id: i64,
    feed_id: i64,
    category: String,
    feed_title: String,
    kind: String,
    content_type: String,
    title: Option<String>,
    url: Option<String>,
    author: Option<String>,
    snippet: Option<String>,
    image_url: Option<String>,
    published_at: Option<String>,
    is_read: bool,
    is_starred: bool,
    reading_time_secs: Option<i64>,
    duration_secs: Option<i64>,
    score: Option<i64>,
    comments_count: Option<i64>,
    upvote_ratio: Option<f64>,
    transcript_status: String,
    has_summary: bool,
    site_url: Option<String>,
    feed_icon_url: Option<String>,
}

#[derive(Serialize)]
struct ItemsPage {
    items: Vec<ItemDto>,
    page: i64,
    page_size: i64,
    total_pages: i64,
    total_count: i64,
}

/// `GET /api/items` - the filtered, sorted, paginated card grid (§10). Offset/limit pagination.
async fn list_items(
    user: CurrentUser,
    State(state): State<AppState>,
    Query(q): Query<ItemQuery>,
) -> ApiResult<Json<ItemsPage>> {
    let tz = user_tz(&state.pool, user.id).await;
    let filters = build_filters(&q, &tz);
    let sort = q.sort.unwrap_or_default();

    let page_size = resolve_page_size(&state.pool, user.id, q.page_size).await;
    let page = q.page.unwrap_or(1).max(1);
    let offset = (page - 1) * page_size;
    let video_route = crate::ai::provider::load_video_route(&state.pool, &state.enc_key).await?;
    let text_route = crate::ai::provider::load_text_route(&state.pool, &state.enc_key).await?;

    // Count (same scope) → total pages.
    let mut cb = QueryBuilder::new("SELECT COUNT(*) AS n");
    push_scope(&mut cb, user.id, &filters);
    let total_count: i64 = cb.build().fetch_one(&state.pool).await?.get("n");
    let total_pages = if total_count == 0 {
        0
    } else {
        (total_count + page_size - 1) / page_size
    };

    // Page of items.
    let mut qb = QueryBuilder::new(
        "SELECT i.id AS id, i.feed_id AS feed_id, i.url AS url, i.title AS title, i.author AS author, \
                i.content_text AS content_text, i.image_url AS image_url, i.published_at AS published_at, \
                i.reading_time_secs AS reading_time_secs, i.duration_secs AS duration_secs, \
                i.score AS score, i.comments_count AS comments_count, i.upvote_ratio AS upvote_ratio, \
                i.transcript_status AS transcript_status, fe.kind AS kind, s.content_type AS content_type, \
                c.name AS category, \
                COALESCE(NULLIF(s.title_override, ''), NULLIF(fe.title, ''), fe.feed_url) AS feed_title, \
                 COALESCE(st.is_read, 0) AS is_read, COALESCE(st.is_starred, 0) AS is_starred, \
                 CASE WHEN fe.kind = 'youtube' THEN (",
    );
    push_summary_exists(&mut qb, &video_route, &text_route, true);
    qb.push(") ELSE (");
    push_summary_exists(&mut qb, &[], &text_route, false);
    qb.push(") END AS has_summary, fe.site_url AS site_url, fe.icon_url AS feed_icon_url");
    push_scope(&mut qb, user.id, &filters);
    qb.push(" ORDER BY ");
    qb.push(sort_clause(&sort)); // whitelisted &'static str - never user input
    qb.push(" LIMIT ");
    qb.push_bind(page_size);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows = qb.build().fetch_all(&state.pool).await?;
    let items = rows.iter().map(row_to_item).collect();

    Ok(Json(ItemsPage {
        items,
        page,
        page_size,
        total_pages,
        total_count,
    }))
}

/// Appends one correlated cache predicate using the routes resolved for this request. Keeping this
/// in the page query avoids treating stale, legacy, or differently-configured rows as summaries.
fn push_summary_exists(
    qb: &mut QueryBuilder<'_, Sqlite>,
    video_route: &[crate::ai::provider::ResolvedProvider],
    text_route: &[crate::ai::provider::ResolvedProvider],
    is_video: bool,
) {
    let mut first = true;
    let mut push_cache = |provider: &crate::ai::provider::ResolvedProvider,
                          summary_kind: &'static str| {
        if !first {
            qb.push(" OR ");
        }
        first = false;
        qb.push(
            "EXISTS (SELECT 1 FROM item_summaries su WHERE su.item_id = i.id AND su.provider_id = ",
        );
        qb.push_bind(provider.id);
        qb.push(" AND su.model = ");
        qb.push_bind(provider.model.clone());
        qb.push(" AND su.summary_kind = ");
        qb.push_bind(summary_kind);
        qb.push(" AND TRIM(su.summary_text) <> '')");
    };

    if is_video {
        for provider in video_route {
            push_cache(provider, "video-topics-v1");
        }
        for provider in text_route {
            push_cache(provider, "text-video-topics-v1");
        }
    } else {
        for provider in text_route {
            push_cache(provider, "text");
        }
    }
    if first {
        qb.push("0");
    }
}

fn row_to_item(r: &sqlx::sqlite::SqliteRow) -> ItemDto {
    ItemDto {
        id: r.get("id"),
        feed_id: r.get("feed_id"),
        category: r.get("category"),
        feed_title: r.get("feed_title"),
        kind: r.get("kind"),
        content_type: r.get("content_type"),
        title: r.get("title"),
        url: r.get("url"),
        author: r.get("author"),
        snippet: snippet(r.get::<Option<String>, _>("content_text")),
        image_url: r.get("image_url"),
        published_at: r.get("published_at"),
        is_read: r.get::<i64, _>("is_read") != 0,
        is_starred: r.get::<i64, _>("is_starred") != 0,
        reading_time_secs: r.get("reading_time_secs"),
        duration_secs: r.get("duration_secs"),
        score: r.get("score"),
        comments_count: r.get("comments_count"),
        upvote_ratio: r.get("upvote_ratio"),
        transcript_status: r.get("transcript_status"),
        has_summary: r.get::<i64, _>("has_summary") != 0,
        site_url: r.get("site_url"),
        feed_icon_url: r.get("feed_icon_url"),
    }
}

/// A 2-line card snippet: collapse whitespace, cap length (§9.1).
fn snippet(text: Option<String>) -> Option<String> {
    let clean = text?.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.is_empty() {
        return None;
    }
    if clean.chars().count() > 240 {
        Some(format!("{}…", clean.chars().take(240).collect::<String>()))
    } else {
        Some(clean)
    }
}

// ---------------------------------------------------------------------------
// GET /api/items/{id}
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ItemDetailDto {
    #[serde(flatten)]
    item: ItemDto,
    content_html: Option<String>,
    transcript_text: Option<String>,
    summary: Option<String>,
    summary_kind: Option<String>,
}

/// `GET /api/items/{id}` - full item for the preview surface (§9.1a). 404 unless the caller
/// subscribes to the item's feed. `summary` is the shared cache entry when present (Phase 5 fills
/// this in; for now it's whatever's cached, else null).
async fn get_item(
    user: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<ItemDetailDto>> {
    let row = sqlx::query(
        "SELECT i.id AS id, i.feed_id AS feed_id, i.url AS url, i.title AS title, i.author AS author, \
                i.content_text AS content_text, i.content_html AS content_html, i.image_url AS image_url, \
                i.published_at AS published_at, i.reading_time_secs AS reading_time_secs, \
                i.duration_secs AS duration_secs, i.score AS score, i.comments_count AS comments_count, \
                i.upvote_ratio AS upvote_ratio, i.transcript_status AS transcript_status, \
                i.transcript_text AS transcript_text, fe.kind AS kind, fe.site_url AS site_url, \
                fe.icon_url AS feed_icon_url, \
                 s.content_type AS content_type, c.name AS category, \
                 COALESCE(NULLIF(s.title_override, ''), NULLIF(fe.title, ''), fe.feed_url) AS feed_title, \
                  COALESCE(st.is_read, 0) AS is_read, COALESCE(st.is_starred, 0) AS is_starred, \
                  0 AS has_summary \
          FROM items i \
          JOIN subscriptions s ON s.feed_id = i.feed_id AND s.user_id = ? \
          JOIN feeds fe ON fe.id = i.feed_id \
          JOIN categories c ON c.id = s.category_id \
          LEFT JOIN item_states st ON st.item_id = i.id AND st.user_id = ? \
          WHERE i.id = ?",
    )
    .bind(user.id)
    .bind(user.id)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
        .ok_or_else(|| AppError::NotFound("item not found".into()))?;

    let kind: String = row.get("kind");
    let summary = if kind == "youtube" {
        let video_route =
            crate::ai::provider::load_video_route(&state.pool, &state.enc_key).await?;
        let mut found = None;
        for provider in video_route {
            if let Some(summary) = cached_summary(
                &state.pool,
                id,
                provider.id,
                &provider.model,
                "video-topics-v1",
            )
            .await?
            {
                found = Some(summary);
                break;
            }
        }
        found
    } else {
        None
    };
    let summary = match summary {
        Some(summary) => Some((summary, "video-topics-v1".to_string())),
        None => {
            let text_route =
                crate::ai::provider::load_text_route(&state.pool, &state.enc_key).await?;
            let mut found = None;
            for provider in text_route {
                let summary_kind = if kind == "youtube" {
                    "text-video-topics-v1"
                } else {
                    "text"
                };
                if let Some(summary) =
                    cached_summary(&state.pool, id, provider.id, &provider.model, summary_kind)
                        .await?
                {
                    found = Some((summary, summary_kind.to_string()));
                    break;
                }
            }
            found
        }
    };

    let mut item = row_to_item(&row);
    item.has_summary = summary.is_some();
    let detail = ItemDetailDto {
        content_html: row.get("content_html"),
        transcript_text: row.get("transcript_text"),
        summary: summary.as_ref().map(|(text, _)| text.clone()),
        summary_kind: summary.map(|(_, kind)| kind),
        item,
    };
    Ok(Json(detail))
}

async fn cached_summary(
    pool: &SqlitePool,
    item_id: i64,
    provider_id: i64,
    model: &str,
    summary_kind: &str,
) -> Result<Option<String>, sqlx::Error> {
    Ok(sqlx::query(
        "SELECT summary_text FROM item_summaries \
          WHERE item_id = ? AND provider_id = ? AND model = ? AND summary_kind = ?
            AND TRIM(summary_text) <> ''",
    )
    .bind(item_id)
    .bind(provider_id)
    .bind(model)
    .bind(summary_kind)
    .fetch_optional(pool)
    .await?
    .map(|r| r.get("summary_text")))
}

// ---------------------------------------------------------------------------
// POST /api/items/{id}/read · /star
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct SetFlag {
    /// Explicit value to set; omitted → toggle current.
    value: Option<bool>,
}

#[derive(Serialize)]
struct StateDto {
    is_read: bool,
    is_starred: bool,
}

/// `POST /api/items/{id}/read` - per-user upsert into `item_states` (no pre-created rows).
async fn set_read(
    user: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    body: Option<Json<SetFlag>>,
) -> ApiResult<Json<StateDto>> {
    owned_item(&state.pool, user.id, id).await?;
    let (is_read, is_starred) = current_state(&state.pool, user.id, id).await?;
    let new_read = body.and_then(|b| b.value).unwrap_or(!is_read);

    sqlx::query(
        "INSERT INTO item_states (user_id, item_id, is_read, is_starred, read_at) \
         VALUES (?, ?, ?, ?, CASE WHEN ? THEN datetime('now') END) \
         ON CONFLICT(user_id, item_id) DO UPDATE SET \
             is_read = excluded.is_read, \
             read_at = CASE WHEN excluded.is_read = 1 THEN datetime('now') END",
    )
    .bind(user.id)
    .bind(id)
    .bind(new_read as i64)
    .bind(is_starred as i64)
    .bind(new_read as i64)
    .execute(&state.pool)
    .await?;

    Ok(Json(StateDto {
        is_read: new_read,
        is_starred,
    }))
}

/// `POST /api/items/{id}/star` - per-user upsert into `item_states`.
async fn set_star(
    user: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    body: Option<Json<SetFlag>>,
) -> ApiResult<Json<StateDto>> {
    owned_item(&state.pool, user.id, id).await?;
    let (is_read, is_starred) = current_state(&state.pool, user.id, id).await?;
    let new_starred = body.and_then(|b| b.value).unwrap_or(!is_starred);

    sqlx::query(
        "INSERT INTO item_states (user_id, item_id, is_read, is_starred, read_at) \
         VALUES (?, ?, ?, ?, CASE WHEN ? THEN datetime('now') END) \
         ON CONFLICT(user_id, item_id) DO UPDATE SET is_starred = excluded.is_starred",
    )
    .bind(user.id)
    .bind(id)
    .bind(is_read as i64)
    .bind(new_starred as i64)
    .bind(is_read as i64)
    .execute(&state.pool)
    .await?;

    Ok(Json(StateDto {
        is_read,
        is_starred: new_starred,
    }))
}

// ---------------------------------------------------------------------------
// POST /api/items/{id}/summarize
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SummarizeQuery {
    /// `?force=1` regenerates even if a summary is cached (prompt.md §6, §10).
    force: Option<String>,
}

#[derive(Serialize)]
struct SummaryDto {
    summary: String,
    summary_kind: String,
    model: String,
    cached: bool,
}

/// `POST /api/items/{id}/summarize` - on-demand AI summary for a reading or video item, written to
/// the shared cache and reused (prompt.md §6, §6a). Scoped: 404 unless the caller subscribes to the
/// item's feed. Any AI/provider/budget failure returns a clear error (never crashes, §11).
async fn summarize(
    user: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<SummarizeQuery>,
) -> ApiResult<Json<SummaryDto>> {
    owned_item(&state.pool, user.id, id).await?;
    let force = matches!(q.force.as_deref(), Some("1") | Some("true"));

    use crate::ai::summarize::{summarize_item, SummarizeError};
    match summarize_item(&state.pool, &state.http_client, &state.enc_key, id, force).await {
        Ok(r) => Ok(Json(SummaryDto {
            summary: r.summary,
            summary_kind: r.summary_kind,
            model: r.model,
            cached: r.cached,
        })),
        Err(SummarizeError::NotConfigured) => Err(AppError::BadRequest(
            "AI summarization is not configured - an admin must add and activate a provider."
                .into(),
        )),
        Err(SummarizeError::NoContent) => Err(AppError::BadRequest(
            "this item has no text to summarize".into(),
        )),
        Err(SummarizeError::Budget(m)) => Err(AppError::BadRequest(m)),
        Err(SummarizeError::Provider(m)) => Err(AppError::Upstream(m)),
        Err(SummarizeError::Internal(e)) => Err(AppError::Internal(e)),
    }
}

// ---------------------------------------------------------------------------
// GET /api/categories/counts
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CountsQuery {
    r#type: Option<String>,
    status: Option<String>,
    when: Option<String>,
}

#[derive(Serialize)]
struct CategoryCount {
    category_id: i64,
    count: i64,
}

#[derive(Serialize)]
struct CountsDto {
    total: i64,
    categories: Vec<CategoryCount>,
}

/// `GET /api/categories/counts` - chip counts that reflect the active Type/Status/When facets but
/// NOT the category facet itself (§9.1, §11). Includes every category (zero-filled) plus a total.
async fn category_counts(
    user: CurrentUser,
    State(state): State<AppState>,
    Query(q): Query<CountsQuery>,
) -> ApiResult<Json<CountsDto>> {
    let tz = user_tz(&state.pool, user.id).await;
    let when = when_range(q.when.as_deref().unwrap_or("all"), &tz, Utc::now());
    let filters = Filters {
        content_type: content_type_filter(q.r#type.as_deref()),
        status: status_filter(q.status.as_deref()),
        when_start: when.start,
        when_end: when.end,
        ..Default::default()
    };

    let mut qb = QueryBuilder::new("SELECT s.category_id AS cid, COUNT(*) AS n");
    push_scope(&mut qb, user.id, &filters);
    qb.push(" GROUP BY s.category_id");
    let rows = qb.build().fetch_all(&state.pool).await?;

    let mut by_id: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
    let mut total = 0i64;
    for r in &rows {
        let cid: i64 = r.get("cid");
        let n: i64 = r.get("n");
        by_id.insert(cid, n);
        total += n;
    }

    // Zero-fill every category the user owns so chips are stable.
    let cats = sqlx::query("SELECT id FROM categories WHERE user_id = ? ORDER BY position, name")
        .bind(user.id)
        .fetch_all(&state.pool)
        .await?;
    let categories = cats
        .iter()
        .map(|r| {
            let id: i64 = r.get("id");
            CategoryCount {
                category_id: id,
                count: by_id.get(&id).copied().unwrap_or(0),
            }
        })
        .collect();

    Ok(Json(CountsDto { total, categories }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn content_type_filter(t: Option<&str>) -> Option<String> {
    match t {
        Some("reading") => Some("reading".into()),
        Some("video") => Some("video".into()),
        _ => None,
    }
}

fn status_filter(s: Option<&str>) -> Option<String> {
    match s {
        Some("unread") => Some("unread".into()),
        Some("starred") => Some("starred".into()),
        _ => None,
    }
}

fn build_filters(q: &ItemQuery, tz: &Tz) -> Filters {
    let when = when_range(q.when.as_deref().unwrap_or("all"), tz, Utc::now());
    let category = q.category.as_deref().and_then(|c| {
        if c == "all" {
            None
        } else {
            i64::from_str(c).ok()
        }
    });
    Filters {
        content_type: content_type_filter(q.r#type.as_deref()),
        status: status_filter(q.status.as_deref()),
        category,
        feed: q.feed,
        when_start: when.start,
        when_end: when.end,
        q: q.q.as_deref().and_then(fts_query),
    }
}

/// The caller's stored timezone preference (per-user `settings`), defaulting to UTC. The Settings
/// UI that writes this lands in Phase 7; until then everyone is effectively UTC.
async fn user_tz(pool: &SqlitePool, user_id: i64) -> Tz {
    let v = user_setting(pool, user_id, "timezone").await;
    parse_tz(v.as_deref())
}

/// Page size: explicit query param wins, else the user's `page_size` preference, else the default
/// (§9.1). Always clamped to a sane range.
async fn resolve_page_size(pool: &SqlitePool, user_id: i64, param: Option<i64>) -> i64 {
    let raw = match param {
        Some(p) => p,
        None => user_setting(pool, user_id, "page_size")
            .await
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_PAGE_SIZE),
    };
    raw.clamp(1, MAX_PAGE_SIZE)
}

async fn user_setting(pool: &SqlitePool, user_id: i64, key: &str) -> Option<String> {
    sqlx::query("SELECT value FROM settings WHERE user_id = ? AND key = ?")
        .bind(user_id)
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .map(|r| r.get("value"))
}

/// Ensure the item exists AND the caller subscribes to its feed (per-user scoping). 404 otherwise.
async fn owned_item(pool: &SqlitePool, user_id: i64, item_id: i64) -> ApiResult<()> {
    let ok = sqlx::query(
        "SELECT 1 FROM items i \
         JOIN subscriptions s ON s.feed_id = i.feed_id AND s.user_id = ? \
         WHERE i.id = ?",
    )
    .bind(user_id)
    .bind(item_id)
    .fetch_optional(pool)
    .await?
    .is_some();
    if ok {
        Ok(())
    } else {
        Err(AppError::NotFound("item not found".into()))
    }
}

/// Current (is_read, is_starred) for a user+item; (false, false) when no row exists yet.
async fn current_state(pool: &SqlitePool, user_id: i64, item_id: i64) -> ApiResult<(bool, bool)> {
    let row = sqlx::query(
        "SELECT is_read, is_starred FROM item_states WHERE user_id = ? AND item_id = ?",
    )
    .bind(user_id)
    .bind(item_id)
    .fetch_optional(pool)
    .await?;
    Ok(match row {
        Some(r) => (
            r.get::<i64, _>("is_read") != 0,
            r.get::<i64, _>("is_starred") != 0,
        ),
        None => (false, false),
    })
}
