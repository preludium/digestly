//! Persistence for ingestion (prompt.md §4 steps 3–5): transactional insert of new items only
//! (dedup + FTS via triggers), plus feed success/failure state with exponential backoff.

use anyhow::Result;
use argon2::password_hash::rand_core::{OsRng, RngCore};
use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};

use super::fetch::{FetchError, Fetched};
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
/// number of newly-inserted items. Dedup by `(feed_id, dedup_hash)` (prompt.md §11). Items
/// published earlier than `max_age_days` ago are never newly inserted (0 = no cutoff) - a feed
/// with a backlog (first poll, or one that was down a while) doesn't dump old items in; an item
/// already tracked still gets content updates regardless of age.
pub async fn insert_items(
    pool: &SqlitePool,
    feed_id: i64,
    items: &[ParsedItem],
    max_age_days: i64,
) -> Result<usize> {
    insert_items_with_auto_summary(pool, feed_id, items, max_age_days, false).await
}

/// Insert new items and mark them for one automatic summary attempt when requested. The marker is
/// applied only in the insert branch; previously stored items never become pending retroactively.
pub async fn insert_items_with_auto_summary(
    pool: &SqlitePool,
    feed_id: i64,
    items: &[ParsedItem],
    max_age_days: i64,
    auto_summary_pending: bool,
) -> Result<usize> {
    let mut tx = pool.begin().await?;
    let mut inserted = 0usize;
    let cutoff = (max_age_days > 0).then(|| Utc::now() - chrono::Duration::days(max_age_days));

    for item in items {
        let existing: Option<(i64, Option<String>)> =
            sqlx::query("SELECT id, content_text FROM items WHERE feed_id = ? AND dedup_hash = ?")
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
                if cutoff.is_some_and(|c| item.published_at < c) {
                    continue;
                }
                sqlx::query(
                    "INSERT INTO items
                        (feed_id, guid, url, title, author, content_html, content_text,
                         transcript_status, image_url, duration_secs, reading_time_secs,
                          published_at, score, comments_count, upvote_ratio, dedup_hash,
                          auto_summary_pending)
                     VALUES (?, ?, ?, ?, ?, ?, ?, 'none', ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
                .bind(auto_summary_pending as i64)
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
pub async fn record_not_modified(
    pool: &SqlitePool,
    feed_id: i64,
    interval_secs: i64,
) -> Result<()> {
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
/// Returns `true` when this poll is the **healthy→failing/disabled transition** - i.e. the feed
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

    sqlx::query("UPDATE feeds SET disabled = ?, next_fetch_at = datetime('now', ?) WHERE id = ?")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    fn item(dedup_hash: &str, published_at: DateTime<Utc>) -> ParsedItem {
        ParsedItem {
            guid: None,
            url: None,
            title: Some("t".into()),
            author: None,
            content_html: None,
            content_text: None,
            image_url: None,
            duration_secs: None,
            reading_time_secs: None,
            published_at,
            score: None,
            comments_count: None,
            upvote_ratio: None,
            dedup_hash: dedup_hash.into(),
        }
    }

    async fn make_feed(pool: &SqlitePool) -> i64 {
        sqlx::query("INSERT INTO feeds (feed_url, kind, fetch_interval_secs) VALUES ('u', 'rss', 3600) RETURNING id")
            .fetch_one(pool)
            .await
            .unwrap()
            .get("id")
    }

    #[tokio::test]
    async fn max_age_days_zero_ingests_everything() {
        let pool = test_pool().await;
        let feed_id = make_feed(&pool).await;
        let old = item("a", Utc::now() - chrono::Duration::days(10));
        let n = insert_items(&pool, feed_id, &[old], 0).await.unwrap();
        assert_eq!(n, 1, "0 = no cutoff, old items still ingested");
    }

    #[tokio::test]
    async fn old_items_are_skipped_new_items_are_kept() {
        let pool = test_pool().await;
        let feed_id = make_feed(&pool).await;
        let old = item("old", Utc::now() - chrono::Duration::days(5));
        let recent = item("recent", Utc::now() - chrono::Duration::hours(1));
        let n = insert_items(&pool, feed_id, &[old, recent], 1)
            .await
            .unwrap();
        assert_eq!(n, 1, "only the item within the 1-day cutoff is inserted");

        let count: i64 = sqlx::query("SELECT COUNT(*) AS c FROM items WHERE feed_id = ?")
            .bind(feed_id)
            .fetch_one(&pool)
            .await
            .unwrap()
            .get("c");
        assert_eq!(
            count, 1,
            "the old item was never stored, not just excluded from the count"
        );
    }

    #[tokio::test]
    async fn already_tracked_items_still_update_regardless_of_age() {
        let pool = test_pool().await;
        let feed_id = make_feed(&pool).await;
        let old_ts = Utc::now() - chrono::Duration::days(5);
        insert_items(&pool, feed_id, &[item("guid:x", old_ts)], 0)
            .await
            .unwrap();

        let mut updated = item("guid:x", old_ts);
        updated.content_text = Some("updated body".into());
        let n = insert_items(&pool, feed_id, &[updated], 1).await.unwrap();
        assert_eq!(n, 0, "not counted as newly inserted");

        let text: Option<String> = sqlx::query(
            "SELECT content_text FROM items WHERE feed_id = ? AND dedup_hash = 'guid:x'",
        )
        .bind(feed_id)
        .fetch_one(&pool)
        .await
        .unwrap()
        .get("content_text");
        assert_eq!(
            text.as_deref(),
            Some("updated body"),
            "content still synced despite the cutoff"
        );
    }

    #[tokio::test]
    async fn auto_summary_marker_is_set_only_on_new_items() {
        let pool = test_pool().await;
        let feed_id = make_feed(&pool).await;
        let published_at = Utc::now();

        insert_items_with_auto_summary(&pool, feed_id, &[item("guid:new", published_at)], 0, true)
            .await
            .unwrap();
        insert_items_with_auto_summary(&pool, feed_id, &[item("guid:old", published_at)], 0, false)
            .await
            .unwrap();
        insert_items_with_auto_summary(&pool, feed_id, &[item("guid:old", published_at)], 0, true)
            .await
            .unwrap();

        let markers: Vec<i64> = sqlx::query(
            "SELECT auto_summary_pending FROM items WHERE feed_id = ? ORDER BY dedup_hash",
        )
        .bind(feed_id)
        .fetch_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.get("auto_summary_pending"))
        .collect();
        assert_eq!(markers, [1, 0]);
    }
}
