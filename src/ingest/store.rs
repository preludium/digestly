//! Persistence for ingestion (prompt.md §4 steps 3–5): transactional insert of new items only
//! (dedup + FTS via triggers), plus feed success/failure state with exponential backoff.

use anyhow::Result;
use argon2::password_hash::rand_core::{OsRng, RngCore};
use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};

use super::fetch::{Fetched, FetchError};
use super::settings::MAX_FAILURES;
use super::{backoff_secs, ParsedFeed, ParsedItem};

/// SQLite `datetime()` text format (UTC), so stored timestamps sort/compare with `datetime('now')`.
fn fmt_dt(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Fill in feed title/site/icon from a parse when they're still empty (don't clobber later edits).
pub async fn apply_feed_metadata(pool: &SqlitePool, feed_id: i64, feed: &ParsedFeed) -> Result<()> {
    sqlx::query(
        "UPDATE feeds
         SET title       = COALESCE(NULLIF(title, ''), ?),
             site_url    = COALESCE(NULLIF(site_url, ''), ?),
             icon_url    = COALESCE(NULLIF(icon_url, ''), ?)
         WHERE id = ?",
    )
    .bind(&feed.title)
    .bind(&feed.site_url)
    .bind(&feed.icon_url)
    .bind(feed_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Insert new items in one transaction; update on GUID match with changed content. Returns the
/// number of newly-inserted items. Dedup by `(feed_id, dedup_hash)` (prompt.md §11).
pub async fn insert_items(pool: &SqlitePool, feed_id: i64, items: &[ParsedItem]) -> Result<usize> {
    let mut tx = pool.begin().await?;
    let mut inserted = 0usize;

    for item in items {
        let existing: Option<(i64, Option<String>)> = sqlx::query(
            "SELECT id, content_text FROM items WHERE feed_id = ? AND dedup_hash = ?",
        )
        .bind(feed_id)
        .bind(&item.dedup_hash)
        .fetch_optional(&mut *tx)
        .await?
        .map(|r| (r.get("id"), r.get("content_text")));

        match existing {
            Some((id, old_text)) => {
                // Same GUID + changed content → update, not new (§11). Trigger re-syncs FTS.
                let guid_based = item.dedup_hash.starts_with("guid:");
                if guid_based && old_text.as_deref() != item.content_text.as_deref() {
                    sqlx::query(
                        "UPDATE items SET title = ?, content_html = ?, content_text = ?,
                                          image_url = ?, reading_time_secs = ?
                         WHERE id = ?",
                    )
                    .bind(&item.title)
                    .bind(&item.content_html)
                    .bind(&item.content_text)
                    .bind(&item.image_url)
                    .bind(item.reading_time_secs)
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
                }
            }
            None => {
                sqlx::query(
                    "INSERT INTO items
                        (feed_id, guid, url, title, author, content_html, content_text,
                         transcript_status, image_url, duration_secs, reading_time_secs,
                         published_at, score, comments_count, upvote_ratio, dedup_hash)
                     VALUES (?, ?, ?, ?, ?, ?, ?, 'none', ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(feed_id)
                .bind(&item.guid)
                .bind(&item.url)
                .bind(&item.title)
                .bind(&item.author)
                .bind(&item.content_html)
                .bind(&item.content_text)
                .bind(&item.image_url)
                .bind(item.duration_secs)
                .bind(item.reading_time_secs)
                .bind(fmt_dt(item.published_at))
                .bind(item.score)
                .bind(item.comments_count)
                .bind(item.upvote_ratio)
                .bind(&item.dedup_hash)
                .execute(&mut *tx)
                .await?;
                inserted += 1;
            }
        }
    }

    tx.commit().await?;
    Ok(inserted)
}

/// Reset failure state, store validators, and schedule the next poll (prompt.md §4 step 4).
pub async fn record_success(
    pool: &SqlitePool,
    feed_id: i64,
    fetched: &Fetched,
    interval_secs: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE feeds
         SET failure_count = 0, last_error = NULL,
             etag = ?, last_modified = ?,
             feed_url = COALESCE(?, feed_url),
             last_fetch_at = datetime('now'),
             next_fetch_at = datetime('now', ?)
         WHERE id = ?",
    )
    .bind(&fetched.etag)
    .bind(&fetched.last_modified)
    .bind(&fetched.permanent_url)
    .bind(format!("+{interval_secs} seconds"))
    .bind(feed_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// 304 Not Modified: nothing changed, just touch + reschedule (prompt.md §4 step 2).
pub async fn record_not_modified(pool: &SqlitePool, feed_id: i64, interval_secs: i64) -> Result<()> {
    sqlx::query(
        "UPDATE feeds
         SET failure_count = 0, last_error = NULL,
             last_fetch_at = datetime('now'),
             next_fetch_at = datetime('now', ?)
         WHERE id = ?",
    )
    .bind(format!("+{interval_secs} seconds"))
    .bind(feed_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Record a failed poll: increment count, store the error, reschedule with backoff, and disable
/// after too many consecutive failures or on a terminal status (prompt.md §4 step 5, §11).
///
/// Returns `true` when this poll is the **healthy→failing/disabled transition** — i.e. the feed
/// was healthy before (`failure_count = 0`, not disabled) and is now failing/disabled. Callers use
/// that to fire a throttled feed-health notification exactly once per transition (§7a, §11); a
/// feed that keeps failing returns `false` on every subsequent poll.
pub async fn record_failure(pool: &SqlitePool, feed_id: i64, err: &FetchError) -> Result<bool> {
    // RETURNING gives the *post-increment* failure_count and the *pre-update* disabled flag (the
    // second UPDATE below sets `disabled`), so we can detect the "was healthy" edge.
    let row = sqlx::query(
        "UPDATE feeds SET failure_count = failure_count + 1, last_error = ?, last_fetch_at = datetime('now')
         WHERE id = ? RETURNING failure_count, disabled",
    )
    .bind(err.message())
    .bind(feed_id)
    .fetch_one(pool)
    .await?;
    let new_count: i64 = row.get("failure_count");
    let was_disabled: i64 = row.get("disabled");

    // Healthy before this poll ⇒ failure_count was 0 (now 1) and it wasn't already disabled.
    let became_unhealthy = new_count == 1 && was_disabled == 0;

    let (disable, delay_secs) = match err {
        FetchError::Disable(_) => (true, backoff_secs(new_count, jitter())),
        FetchError::RetryAfter(secs, _) => (new_count >= MAX_FAILURES, *secs),
        FetchError::Transient(_) => (new_count >= MAX_FAILURES, backoff_secs(new_count, jitter())),
    };

    sqlx::query(
        "UPDATE feeds SET disabled = ?, next_fetch_at = datetime('now', ?) WHERE id = ?",
    )
    .bind(disable as i64)
    .bind(format!("+{delay_secs} seconds"))
    .bind(feed_id)
    .execute(pool)
    .await?;
    Ok(became_unhealthy)
}

/// Uniform random fraction in `[0, 1)` for backoff jitter (OsRng, no extra dependency).
fn jitter() -> f64 {
    (OsRng.next_u32() as f64) / (u32::MAX as f64 + 1.0)
}
