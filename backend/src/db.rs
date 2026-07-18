//! SQLite connection pool + migrations (prompt.md §11 "Ops & data").
//!
//! WAL mode, `busy_timeout`, foreign keys on. Migrations run on boot. Later phases add
//! more migration files under `/migrations`; they are embedded at compile time by
//! `sqlx::migrate!` - no live DB needed to build (Global Rule #2).

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;

/// Open (creating if needed) the SQLite pool with the ops-friendly pragmas.
pub async fn connect(db_path: &Path) -> Result<SqlitePool> {
    let url = format!("sqlite://{}", db_path.display());

    let opts = SqliteConnectOptions::from_str(&url)
        .with_context(|| format!("invalid sqlite path: {}", db_path.display()))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5))
        .foreign_keys(true);

    // SQLite is single-writer; a small pool keeps reads concurrent under WAL.
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(opts)
        .await
        .context("failed to open SQLite database")?;

    Ok(pool)
}

/// Run all embedded migrations. Idempotent.
pub async fn migrate(pool: &SqlitePool) -> Result<()> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .context("database migration failed")?;
    Ok(())
}

/// Cheap liveness probe used by `/api/health`.
pub async fn ping(pool: &SqlitePool) -> bool {
    sqlx::query("SELECT 1").execute(pool).await.is_ok()
}

/// A migrated, throwaway SQLite pool for tests. The `TempDir` is leaked on purpose: dropping it
/// deletes the directory out from under the still-open pool, and the OS cleans `/tmp` anyway.
#[cfg(test)]
pub async fn test_pool() -> SqlitePool {
    let dir = tempfile::tempdir().unwrap();
    let pool = connect(&dir.path().join("test.db")).await.unwrap();
    migrate(&pool).await.unwrap();
    std::mem::forget(dir);
    pool
}

#[cfg(test)]
mod tests {
    use sqlx::{Row, SqlitePool};

    #[tokio::test]
    async fn ai_routing_migration_reuses_legacy_rows_only_for_the_active_provider() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::raw_sql(
            "CREATE TABLE items (id INTEGER PRIMARY KEY);
             CREATE TABLE ai_providers (
                  id INTEGER PRIMARY KEY AUTOINCREMENT,
                  model TEXT NOT NULL,
                  api_style TEXT NOT NULL,
                  is_active INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE item_summaries (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 item_id INTEGER NOT NULL,
                 model TEXT NOT NULL,
                 api_style TEXT NOT NULL,
                 summary_text TEXT NOT NULL,
                 created_at TEXT NOT NULL DEFAULT (datetime('now')),
                 UNIQUE (item_id, model)
             );
             INSERT INTO items (id) VALUES (1), (2);
             INSERT INTO ai_providers (id, model, api_style, is_active)
                  VALUES (10, 'matched', 'openai', 1), (11, 'matched', 'openai', 1),
                         (12, 'other', 'anthropic', 0);
             INSERT INTO item_summaries (item_id, model, api_style, summary_text)
                 VALUES (1, 'matched', 'openai', 'matching cache'),
                        (2, 'unmatched', 'openai', 'unmatched cache'),
                        (2, 'other', 'anthropic', 'second cache');",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::raw_sql(include_str!("../migrations/0004_ai_routing.sql"))
            .execute(&pool)
            .await
            .unwrap();

        let rows = sqlx::query(
            "SELECT item_id, provider_id, model, summary_text
             FROM item_summaries ORDER BY item_id, provider_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].get::<i64, _>("item_id"), 1);
        assert_eq!(rows[0].get::<i64, _>("provider_id"), 10);
        assert_eq!(rows[0].get::<String, _>("summary_text"), "matching cache");
        assert_eq!(rows[1].get::<i64, _>("provider_id"), -3);
        assert_eq!(rows[2].get::<i64, _>("provider_id"), -2);
    }

    #[tokio::test]
    async fn video_topic_summary_kinds_migration_preserves_all_rows() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::raw_sql(
            "CREATE TABLE items (id INTEGER PRIMARY KEY);
             CREATE TABLE item_summaries (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
                 provider_id INTEGER NOT NULL,
                 summary_kind TEXT NOT NULL CHECK (summary_kind IN ('text', 'video')),
                 model TEXT NOT NULL,
                 api_style TEXT NOT NULL CHECK (api_style IN ('openai', 'anthropic')),
                 summary_text TEXT NOT NULL,
                 created_at TEXT NOT NULL DEFAULT (datetime('now')),
                 UNIQUE (item_id, provider_id, summary_kind)
             );
             INSERT INTO items (id) VALUES (1);
             INSERT INTO item_summaries
                 (id, item_id, provider_id, summary_kind, model, api_style, summary_text, created_at)
             VALUES
                 (7, 1, 5, 'text', 'text-model', 'openai', 'legacy text', '2024-01-02 03:04:05'),
                 (8, 1, 6, 'video', 'video-model', 'anthropic', 'legacy video', '2024-02-03 04:05:06'),
                 (9, 1, -9, 'text', 'old-model', 'openai', 'negative provider', '2024-03-04 05:06:07');",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::raw_sql(include_str!(
            "../migrations/0005_video_topic_summary_kinds.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();

        let rows = sqlx::query(
            "SELECT id, provider_id, summary_kind, summary_text, created_at
             FROM item_summaries ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].get::<i64, _>("id"), 7);
        assert_eq!(rows[0].get::<String, _>("summary_kind"), "text");
        assert_eq!(rows[1].get::<String, _>("summary_kind"), "video");
        assert_eq!(rows[2].get::<i64, _>("provider_id"), -9);
        assert_eq!(
            rows[2].get::<String, _>("summary_text"),
            "negative provider"
        );
        assert_eq!(
            rows[2].get::<String, _>("created_at"),
            "2024-03-04 05:06:07"
        );

        for kind in ["video-topics-v1", "text-video-topics-v1"] {
            sqlx::query(
                "INSERT INTO item_summaries (item_id, provider_id, summary_kind, model, api_style, summary_text)
                 VALUES (1, ?, ?, 'm', 'openai', 'current')",
            )
            .bind(if kind == "video-topics-v1" { 10 } else { 11 })
            .bind(kind)
            .execute(&pool)
            .await
            .unwrap();
        }
        assert!(sqlx::query(
            "INSERT INTO item_summaries (item_id, provider_id, summary_kind, model, api_style, summary_text)
             VALUES (1, 12, 'invalid', 'm', 'openai', 'no')",
        )
        .execute(&pool)
        .await
        .is_err());
    }
}
