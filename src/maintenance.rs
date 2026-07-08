//! Periodic maintenance: retention purge (prompt.md §5, §8, §11). Old and over-cap items are
//! deleted so the SQLite file stays small on a Pi — but **starred items are kept forever** (an item
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
    const KEEP_STARRED: &str = "AND id NOT IN (SELECT item_id FROM item_states WHERE is_starred = 1)";

    if policy.max_age_days > 0 {
        let sql = format!(
            "DELETE FROM items WHERE published_at < datetime('now', ?) {KEEP_STARRED}"
        );
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
        loop {
            // Wait first so boot isn't slowed; then purge on each interval.
            tokio::time::sleep(INTERVAL).await;
            if let Err(e) = purge(&pool).await {
                warn!(error = %e, "retention purge failed");
            }
        }
    })
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
