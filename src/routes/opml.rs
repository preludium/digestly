//! OPML import/export endpoints (prompt.md §9.5, §9.7, §10). Import is upload → preview → confirm:
//! POST with `opml` returns a preview (no writes); POST with `items` performs the subscribe. Each
//! imported feed gets a category (its OPML folder, resolved/created for the user; default `Other`),
//! so the round-trip is lossless (§11). Everything is scoped to the session user.

use axum::extract::State;
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::auth::extract::CurrentUser;
use crate::error::{ApiResult, AppError};
use crate::http::AppState;
use crate::ingest::settings::IngestSettings;
use crate::ingest::FeedKind;
use crate::opml::{self, OpmlFeed};
use crate::routes::feeds::subscribe_url;
use crate::seed::OTHER_CATEGORY;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/opml/import", post(import))
        .route("/opml/export", get(export))
}

#[derive(Deserialize)]
struct ImportBody {
    /// Preview mode: raw OPML text to parse.
    opml: Option<String>,
    /// Confirm mode: the (possibly edited) entries to subscribe.
    items: Option<Vec<ImportItem>>,
}

#[derive(Deserialize)]
struct ImportItem {
    feed_url: String,
    title: Option<String>,
    kind: Option<String>,
    /// Category name (its OPML folder). Resolved/created for the user; default `Other`.
    category: Option<String>,
}

#[derive(Serialize)]
struct PreviewEntry {
    feed_url: String,
    title: Option<String>,
    kind: String,
    category: Option<String>,
    already_subscribed: bool,
}

/// `POST /api/opml/import` - preview (`opml`) or confirm (`items`).
async fn import(
    user: CurrentUser,
    State(state): State<AppState>,
    Json(body): Json<ImportBody>,
) -> ApiResult<Json<serde_json::Value>> {
    if let Some(items) = body.items {
        // Confirm: subscribe each entry, resolving/creating its category (default Other).
        let cfg = IngestSettings::load(&state.pool).await;
        let mut imported = 0usize;
        let mut skipped = 0usize;
        for it in items {
            let cat_id =
                resolve_or_create_category(&state.pool, user.id, it.category.as_deref()).await?;
            let kind = FeedKind::from_db(it.kind.as_deref().unwrap_or("rss"));
            let added = subscribe_url(
                &state.pool,
                &cfg,
                user.id,
                &it.feed_url,
                kind,
                cat_id,
                it.title.as_deref(),
                true,
            )
            .await?;
            if added {
                imported += 1;
            } else {
                skipped += 1;
            }
        }
        if imported > 0 {
            state.ingest_trigger.notify_one();
        }
        return Ok(Json(
            serde_json::json!({ "imported": imported, "skipped": skipped }),
        ));
    }

    let Some(text) = body.opml else {
        return Err(AppError::BadRequest(
            "provide `opml` (preview) or `items` (confirm)".into(),
        ));
    };
    let feeds = opml::parse(&text);
    if feeds.is_empty() {
        return Err(AppError::BadRequest(
            "no feeds found in the OPML file".into(),
        ));
    }
    let mut entries = Vec::with_capacity(feeds.len());
    for f in feeds {
        let already =
            crate::routes::feeds::is_subscribed(&state.pool, user.id, &f.feed_url).await?;
        entries.push(PreviewEntry {
            already_subscribed: already,
            feed_url: f.feed_url,
            title: f.title,
            kind: f.kind,
            category: f.category,
        });
    }
    Ok(Json(serde_json::json!({ "entries": entries })))
}

/// `GET /api/opml/export` - the user's subscriptions grouped by category as an OPML download.
async fn export(user: CurrentUser, State(state): State<AppState>) -> ApiResult<impl IntoResponse> {
    let rows = sqlx::query(
        "SELECT c.name AS category, c.position AS pos, f.feed_url AS feed_url, f.kind AS kind,
                f.site_url AS site_url,
                COALESCE(NULLIF(s.title_override, ''), NULLIF(f.title, ''), f.feed_url) AS title
         FROM subscriptions s
         JOIN feeds f ON f.id = s.feed_id
         JOIN categories c ON c.id = s.category_id
         WHERE s.user_id = ?
         ORDER BY c.position, c.name, title",
    )
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;

    // Group preserving category order.
    let mut groups: Vec<(String, Vec<OpmlFeed>)> = Vec::new();
    for r in &rows {
        let category: String = r.get("category");
        let feed = OpmlFeed {
            title: Some(r.get::<String, _>("title")),
            feed_url: r.get("feed_url"),
            html_url: r.get("site_url"),
            kind: r.get("kind"),
            category: Some(category.clone()),
        };
        match groups.iter_mut().find(|(n, _)| n == &category) {
            Some((_, v)) => v.push(feed),
            None => groups.push((category, vec![feed])),
        }
    }

    let xml = opml::build(&user.username, &groups);
    Ok((
        [
            (CONTENT_TYPE, "text/x-opml; charset=utf-8".to_string()),
            (
                CONTENT_DISPOSITION,
                "attachment; filename=\"digestly.opml\"".to_string(),
            ),
        ],
        xml,
    ))
}

/// Resolve a category by name for the user (case-insensitive), creating it if missing. `None`/empty
/// → the protected `Other` catch-all. This is what makes an OPML round-trip lossless.
async fn resolve_or_create_category(
    pool: &SqlitePool,
    user_id: i64,
    name: Option<&str>,
) -> ApiResult<i64> {
    // Blank/absent or oversized (mirrors the categories endpoint's 40-char cap) → the Other catch-all.
    let target = match name
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.chars().count() <= 40)
    {
        Some(n) => n,
        None => OTHER_CATEGORY,
    };

    if let Some(row) =
        sqlx::query("SELECT id FROM categories WHERE user_id = ? AND name = ? COLLATE NOCASE")
            .bind(user_id)
            .bind(target)
            .fetch_optional(pool)
            .await?
    {
        return Ok(row.get("id"));
    }

    let position: i64 =
        sqlx::query("SELECT COALESCE(MAX(position), 0) + 1 AS p FROM categories WHERE user_id = ?")
            .bind(user_id)
            .fetch_one(pool)
            .await?
            .get("p");
    let id: i64 = sqlx::query(
        "INSERT INTO categories (user_id, name, position) VALUES (?, ?, ?) RETURNING id",
    )
    .bind(user_id)
    .bind(target)
    .bind(position)
    .fetch_one(pool)
    .await?
    .get("id");
    Ok(id)
}
