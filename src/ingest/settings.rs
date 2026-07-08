//! Ingestion tunables (prompt.md §8 "Admin-only ingestion"). Stored in `app_settings` so the
//! Phase 7 admin UI can change them without env/redeploys; code holds the defaults. No env vars.

use sqlx::{Row, SqlitePool};

/// Descriptive UA — Reddit blocks generic/empty agents (prompt.md §3).
pub const USER_AGENT: &str =
    "Digestly/0.1 (+https://github.com/digestly/digestly; self-hosted feed reader)";

/// Consecutive-failure threshold before a feed is auto-disabled (prompt.md §4).
pub const MAX_FAILURES: i64 = 6;

/// Backoff cap (~6h, prompt.md §4).
pub const BACKOFF_CAP_SECS: i64 = 6 * 3600;

/// Resolved ingestion configuration for one scheduler tick.
#[derive(Clone, Debug)]
pub struct IngestSettings {
    pub concurrency: usize,
    pub per_host_delay_ms: u64,
    pub timeout_secs: u64,
    pub default_interval_secs: i64,
    pub allow_private: bool,
    /// Max response body read, in bytes (giant-feed guard).
    pub body_cap_bytes: usize,
    /// Max stored item content length, in chars (giant-item guard).
    pub item_content_cap: usize,
}

impl Default for IngestSettings {
    fn default() -> Self {
        Self {
            concurrency: 8,
            per_host_delay_ms: 1500,
            timeout_secs: 20,
            default_interval_secs: 3600,
            allow_private: false,
            body_cap_bytes: 5 * 1024 * 1024,
            item_content_cap: 200_000,
        }
    }
}

impl IngestSettings {
    /// Load overrides from `app_settings`, falling back to defaults for any unset key.
    pub async fn load(pool: &SqlitePool) -> Self {
        let d = IngestSettings::default();
        Self {
            concurrency: get_int(pool, "ingest.concurrency", d.concurrency as i64).await.max(1) as usize,
            per_host_delay_ms: get_int(pool, "ingest.per_host_delay_ms", d.per_host_delay_ms as i64).await.max(0) as u64,
            timeout_secs: get_int(pool, "ingest.timeout_secs", d.timeout_secs as i64).await.clamp(1, 120) as u64,
            default_interval_secs: get_int(pool, "ingest.default_interval_secs", d.default_interval_secs).await.max(60),
            allow_private: get_bool(pool, "ingest.allow_private", d.allow_private).await,
            body_cap_bytes: get_int(pool, "ingest.body_cap_bytes", d.body_cap_bytes as i64).await.max(4096) as usize,
            item_content_cap: get_int(pool, "ingest.item_content_cap", d.item_content_cap as i64).await.max(1000) as usize,
        }
    }
}

async fn get_str(pool: &SqlitePool, key: &str) -> Option<String> {
    sqlx::query("SELECT value FROM app_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .map(|r| r.get::<String, _>("value"))
}

async fn get_int(pool: &SqlitePool, key: &str, default: i64) -> i64 {
    get_str(pool, key).await.and_then(|v| v.parse().ok()).unwrap_or(default)
}

async fn get_bool(pool: &SqlitePool, key: &str, default: bool) -> bool {
    get_str(pool, key).await.map(|v| v == "true" || v == "1").unwrap_or(default)
}
