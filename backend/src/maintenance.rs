//! Periodic maintenance: retention purge (prompt.md §5, §8, §11). Old and over-cap items are
//! deleted so the SQLite file stays small on a Pi - but **starred items are kept forever** (an item
//! starred by *any* user is never purged). Deletes cascade to `item_states`/`item_summaries` and
//! keep FTS in sync via the `items_ad` trigger.

use anyhow::Result;
use sqlx::{Row, SqlitePool};
use tracing::{info, warn};

/// How often the maintenance task runs.
const INTERVAL: std::time::Duration = std::time::Duration::from_secs(6 * 3600);

/// Retention policy from `app_settings` (both `0` = unlimited/keep-forever).
#[derive(Debug, Clone, Copy)]
pub struct RetentionPolicy {
    pub max_age_days: i64,
    pub max_per_feed: i64,
}

impl RetentionPolicy {
    pub async fn load(pool: &SqlitePool) -> Self {
        Self {
            max_age_days: get_int(pool, "retention.max_age_days").await.max(0),
            max_per_feed: get_int(pool, "retention.max_per_feed").await.max(0),
        }
    }
}

/// Apply the retention policy once. Returns the number of items purged. Never touches starred items.
pub async fn purge(pool: &SqlitePool) -> Result<u64> {
    let policy = RetentionPolicy::load(pool).await;
    let mut removed = 0u64;

    // Never purge an item starred by any user.
    const KEEP_STARRED: &str =
        "AND id NOT IN (SELECT item_id FROM item_states WHERE is_starred = 1)";

    if policy.max_age_days > 0 {
        let sql =
            format!("DELETE FROM items WHERE published_at < datetime('now', ?) {KEEP_STARRED}");
        let n = sqlx::query(&sql)
            .bind(format!("-{} days", policy.max_age_days))
            .execute(pool)
            .await?
            .rows_affected();
        removed += n;
    }

    if policy.max_per_feed > 0 {
        // Keep the newest M per feed (by published_at); delete the rest unless starred.
        let sql = format!(
            "DELETE FROM items WHERE id IN (
                SELECT id FROM (
                    SELECT id, ROW_NUMBER() OVER (
                        PARTITION BY feed_id ORDER BY published_at DESC, id DESC
                    ) AS rn FROM items
                ) WHERE rn > ?
             ) {KEEP_STARRED}"
        );
        let n = sqlx::query(&sql)
            .bind(policy.max_per_feed)
            .execute(pool)
            .await?
            .rows_affected();
        removed += n;
    }

    if removed > 0 {
        info!(removed, "retention purge complete");
    }
    Ok(removed)
}

/// Spawn the periodic maintenance loop (retention). Returns a handle aborted on shutdown.
pub fn spawn(pool: SqlitePool) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!("maintenance task started");
        if let Err(e) = reflow_transcripts_once(&pool).await {
            warn!(error = %e, "transcript reflow failed");
        }
        loop {
            // Wait first so boot isn't slowed; then purge on each interval.
            tokio::time::sleep(INTERVAL).await;
            if let Err(e) = purge(&pool).await {
                warn!(error = %e, "retention purge failed");
            }
        }
    })
}

/// Flag key guarding the one-shot transcript reflow.
const TRANSCRIPT_REFLOW_KEY: &str = "maintenance.transcript_reflow_v1";

/// One-shot reflow of stored transcripts: fetches before the readable-transcript change stored
/// one caption cue per line with YouTube's double-encoded entities half-decoded (literal
/// `&#39;` in the UI). Rewrites them via `ai::transcript::readable_transcript` (pure text
/// transform, no network), then sets a flag so this never runs again. Returns rows rewritten.
pub async fn reflow_transcripts_once(pool: &SqlitePool) -> Result<u64> {
    if get_int(pool, TRANSCRIPT_REFLOW_KEY).await == 1 {
        return Ok(0);
    }
    let rows = sqlx::query(
        "SELECT id, transcript_text FROM items
         WHERE transcript_text IS NOT NULL AND transcript_text != ''",
    )
    .fetch_all(pool)
    .await?;
    let mut reflowed = 0u64;
    for row in rows {
        let id: i64 = row.get("id");
        let text: String = row.get("transcript_text");
        let readable = crate::ai::transcript::readable_transcript(&text);
        if readable != text {
            sqlx::query("UPDATE items SET transcript_text = ? WHERE id = ?")
                .bind(&readable)
                .bind(id)
                .execute(pool)
                .await?;
            reflowed += 1;
        }
    }
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES (?, '1')
         ON CONFLICT(key) DO UPDATE SET value = '1'",
    )
    .bind(TRANSCRIPT_REFLOW_KEY)
    .execute(pool)
    .await?;
    if reflowed > 0 {
        info!(reflowed, "stored transcripts reflowed for readability");
    }
    Ok(reflowed)
}

async fn get_int(pool: &SqlitePool, key: &str) -> i64 {
    sqlx::query("SELECT value FROM app_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.get::<String, _>("value").parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    #[tokio::test]
    async fn transcript_reflow_runs_once_and_reflows_stored_text() {
        let pool = test_pool().await;
        let feed_id: i64 = sqlx::query(
            "INSERT INTO feeds (feed_url, kind, fetch_interval_secs) VALUES ('https://example.com/yt', 'youtube', 3600) RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap()
        .get("id");
        let stored =
            "GPT 5.6 Soul just came out and right now\neveryone&#39;s asking the same thing.";
        let item: i64 = sqlx::query(
            "INSERT INTO items (feed_id, dedup_hash, transcript_status, transcript_text, published_at)
             VALUES (?, 'v1', 'fetched', ?, datetime('now')) RETURNING id",
        )
        .bind(feed_id)
        .bind(stored)
        .fetch_one(&pool)
        .await
        .unwrap()
        .get("id");

        let reflowed = reflow_transcripts_once(&pool).await.unwrap();
        assert_eq!(reflowed, 1);
        let text: String = sqlx::query("SELECT transcript_text FROM items WHERE id = ?")
            .bind(item)
            .fetch_one(&pool)
            .await
            .unwrap()
            .get("transcript_text");
        assert_eq!(
            text,
            "GPT 5.6 Soul just came out and right now everyone's asking the same thing."
        );

        // Second run is a guarded no-op: a freshly inserted ragged row stays untouched.
        sqlx::query(
            "INSERT INTO items (feed_id, dedup_hash, transcript_status, transcript_text, published_at)
             VALUES (?, 'v2', 'fetched', 'line one\nline&#39;two', datetime('now'))",
        )
        .bind(feed_id)
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(reflow_transcripts_once(&pool).await.unwrap(), 0);
    }
}
