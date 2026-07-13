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
