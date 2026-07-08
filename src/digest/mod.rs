//! Digest engine (prompt.md §6 "Digest", §7). The **engine is global/admin-configured** (one cron
//! schedule, look-back window, enable, categories, AI on/off) but **content is per-user**: each run
//! iterates users and builds each one a digest of *their* subscriptions grouped *by their*
//! categories, one AI prompt per non-empty category via the active provider, archived to their
//! `digests` row and pushed to their own ntfy channel (§7a).
//!
//! **AI fallback** (§6, §11): a provider error / budget exceeded produces a digest with raw grouped
//! titles + links and a note — it **never** fails the run.

pub mod cron;

use anyhow::Result;
use chrono::{Duration, Utc};
use reqwest::Client;
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};
use tracing::{info, warn};

use crate::ai::provider::{self, ResolvedProvider};
use crate::ai::{budget, client, AiParams, LlmRequest};
use crate::notify;

/// Cap on the number of items whose titles are fed to the per-category AI prompt.
const MAX_ITEMS_PER_CATEGORY_PROMPT: usize = 40;
/// A user with more than this many failed sources in the window gets an explicit alert (§7, §11).
const FAILED_SOURCES_ALERT_THRESHOLD: i64 = 2;

// ---------------------------------------------------------------------------
// Config (global, admin-only; stored in app_settings)
// ---------------------------------------------------------------------------

/// Which categories a digest includes.
#[derive(Debug, Clone, PartialEq)]
pub enum CategoryFilter {
    All,
    Names(Vec<String>),
}

impl CategoryFilter {
    fn to_setting(&self) -> String {
        match self {
            CategoryFilter::All => "all".to_string(),
            CategoryFilter::Names(names) => names.join(","),
        }
    }
    fn from_setting(v: &str) -> CategoryFilter {
        if v.trim().is_empty() || v.trim() == "all" {
            CategoryFilter::All
        } else {
            CategoryFilter::Names(v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        }
    }
    fn includes(&self, name: &str) -> bool {
        match self {
            CategoryFilter::All => true,
            CategoryFilter::Names(names) => names.iter().any(|n| n.eq_ignore_ascii_case(name)),
        }
    }
}

/// The digest engine configuration (prompt.md §7, §9.7 Digest tab).
#[derive(Debug, Clone)]
pub struct DigestConfig {
    pub enabled: bool,
    pub cron: String,
    pub lookback_days: i64,
    pub timezone: String,
    pub categories: CategoryFilter,
    pub ai_enabled: bool,
}

impl Default for DigestConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cron: "0 9 * * *".to_string(), // daily, 09:00 (§7)
            lookback_days: 1,              // 24h — lookback is stored in whole days
            timezone: "UTC".to_string(),
            categories: CategoryFilter::All,
            ai_enabled: true,
        }
    }
}

impl DigestConfig {
    pub async fn load(pool: &SqlitePool) -> Self {
        let d = DigestConfig::default();
        DigestConfig {
            enabled: get_bool(pool, "digest.enabled", d.enabled).await,
            cron: get_str(pool, "digest.cron").await.filter(|c| cron::Cron::parse(c).is_some()).unwrap_or(d.cron),
            lookback_days: get_int(pool, "digest.lookback_days", d.lookback_days).await.clamp(1, 90),
            timezone: get_str(pool, "digest.timezone").await.unwrap_or(d.timezone),
            categories: get_str(pool, "digest.categories").await.map(|v| CategoryFilter::from_setting(&v)).unwrap_or(d.categories),
            ai_enabled: get_bool(pool, "digest.ai_enabled", d.ai_enabled).await,
        }
    }

    pub async fn save(&self, pool: &SqlitePool) -> Result<()> {
        set_setting(pool, "digest.enabled", if self.enabled { "true" } else { "false" }).await?;
        set_setting(pool, "digest.cron", &self.cron).await?;
        set_setting(pool, "digest.lookback_days", &self.lookback_days.clamp(1, 90).to_string()).await?;
        set_setting(pool, "digest.timezone", &self.timezone).await?;
        set_setting(pool, "digest.categories", &self.categories.to_setting()).await?;
        set_setting(pool, "digest.ai_enabled", if self.ai_enabled { "true" } else { "false" }).await?;
        Ok(())
    }

    /// A human-readable schedule preview for the UI (§9.7). Notes when the engine is off.
    pub fn schedule_preview(&self) -> String {
        let base = cron::Cron::parse(&self.cron)
            .map(|c| c.describe())
            .unwrap_or_else(|| "invalid schedule".to_string());
        if self.enabled {
            format!("{base} ({})", self.timezone)
        } else {
            format!("Disabled — {base} ({})", self.timezone)
        }
    }
}

// ---------------------------------------------------------------------------
// Running a digest for every user
// ---------------------------------------------------------------------------

/// Summary of one engine run.
#[derive(Debug, Default, serde::Serialize)]
pub struct RunSummary {
    pub users: usize,
    pub digests: usize,
    pub pushed: usize,
}

/// Run the digest for **all** users (manual admin trigger or scheduled). Never fails on a single
/// user's error — logs and continues. `lookback_override`, when `Some`, replaces the configured
/// `lookback_days` for this run only (e.g. an admin's one-off "last month" manual run) — it is
/// never persisted. The scheduled run always passes `None`.
pub async fn run_all(pool: &SqlitePool, http: &Client, enc_key: &[u8; 32], lookback_override: Option<i64>) -> Result<RunSummary> {
    let cfg = DigestConfig::load(pool).await;
    let lookback_days = lookback_override.map(|d| d.clamp(1, 90)).unwrap_or(cfg.lookback_days);
    let now = Utc::now();
    let period_start = now - Duration::days(lookback_days);
    let start_s = fmt_dt(period_start);
    let end_s = fmt_dt(now);

    // Resolve the active provider + params once for the whole run (shared across users).
    let provider = if cfg.ai_enabled {
        provider::load_active(pool, enc_key).await.unwrap_or(None)
    } else {
        None
    };
    let params = AiParams::load(pool).await;

    let user_ids: Vec<i64> = sqlx::query("SELECT id FROM users ORDER BY id")
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| r.get::<i64, _>("id"))
        .collect();

    let mut summary = RunSummary { users: user_ids.len(), ..Default::default() };
    for user_id in user_ids {
        match build_and_archive(pool, http, enc_key, user_id, &cfg, &provider, &params, &start_s, &end_s).await {
            Ok(pushed) => {
                summary.digests += 1;
                if pushed {
                    summary.pushed += 1;
                }
            }
            Err(e) => warn!(user_id, error = %e, "digest build failed for user (continuing)"),
        }
    }
    info!(users = summary.users, digests = summary.digests, pushed = summary.pushed, "digest run complete");
    Ok(summary)
}

/// An item that belongs in a category section of the digest.
struct DigestItem {
    title: String,
    url: Option<String>,
    feed_title: String,
    published_at: Option<String>,
}

/// Build one user's digest, archive it, and (best-effort) push the ntfy summary. Returns whether a
/// push was sent.
#[allow(clippy::too_many_arguments)]
async fn build_and_archive(
    pool: &SqlitePool,
    http: &Client,
    enc_key: &[u8; 32],
    user_id: i64,
    cfg: &DigestConfig,
    provider: &Option<ResolvedProvider>,
    params: &AiParams,
    start_s: &str,
    end_s: &str,
) -> Result<bool> {
    // 1. Gather the user's in-window items grouped by their categories (ordered by category pos).
    let rows = sqlx::query(
        "SELECT c.name AS category, c.position AS cat_pos, i.title AS title, i.url AS url,
                i.published_at AS published_at,
                COALESCE(NULLIF(s.title_override, ''), NULLIF(fe.title, ''), fe.feed_url) AS feed_title
         FROM items i
         JOIN subscriptions s ON s.feed_id = i.feed_id AND s.user_id = ?
         JOIN feeds fe ON fe.id = i.feed_id
         JOIN categories c ON c.id = s.category_id
         WHERE s.disabled = 0
           AND (i.score IS NULL OR i.score >= s.min_score)
           AND i.published_at >= ? AND i.published_at <= ?
         ORDER BY c.position, c.name, i.published_at DESC",
    )
    .bind(user_id)
    .bind(start_s)
    .bind(end_s)
    .fetch_all(pool)
    .await?;

    // Group by category name, preserving DB order (category position). Only included categories.
    let mut sections: Vec<(String, Vec<DigestItem>)> = Vec::new();
    let mut sources: Vec<String> = Vec::new();
    for r in &rows {
        let category: String = r.get("category");
        if !cfg.categories.includes(&category) {
            continue;
        }
        let feed_title: String = r.get("feed_title");
        if !sources.contains(&feed_title) {
            sources.push(feed_title.clone());
        }
        let item = DigestItem {
            title: r.get::<Option<String>, _>("title").unwrap_or_else(|| "(untitled)".into()),
            url: r.get("url"),
            feed_title,
            published_at: r.get("published_at"),
        };
        match sections.iter_mut().find(|(name, _)| name == &category) {
            Some((_, items)) => items.push(item),
            None => sections.push((category, vec![item])),
        }
    }

    let item_count: i64 = sections.iter().map(|(_, v)| v.len() as i64).sum();
    let failed_sources = failed_source_count(pool, user_id).await.unwrap_or(0);

    // 2. Build each category section: AI summary, or raw fallback on any AI problem (§6, §11).
    let mut ai_used = false;
    let mut used_raw_fallback = false;
    let mut category_json: Vec<Value> = Vec::new();
    let mut category_counts: Vec<(String, i64)> = Vec::new();

    for (name, items) in &sections {
        category_counts.push((name.clone(), items.len() as i64));

        let mut ai_summary: Option<String> = None;
        if cfg.ai_enabled {
            if let Some(p) = provider {
                match summarize_category(pool, http, p, params, name, items).await {
                    Ok(text) => {
                        ai_used = true;
                        ai_summary = Some(text);
                    }
                    Err(reason) => {
                        used_raw_fallback = true;
                        warn!(user_id, category = %name, reason = %reason, "digest AI failed — raw fallback for this category");
                    }
                }
            } else {
                used_raw_fallback = true;
            }
        }

        let raw = ai_summary.is_none();
        category_json.push(json!({
            "name": name,
            "ai_summary": ai_summary,
            "raw": raw,
            "items": items.iter().map(|it| json!({
                "title": it.title,
                "url": it.url,
                "feed_title": it.feed_title,
                "published_at": it.published_at,
            })).collect::<Vec<_>>(),
        }));
    }

    // A fallback note when AI was requested but at least one section couldn't use it.
    let fallback_note: Option<String> = if cfg.ai_enabled && used_raw_fallback {
        Some(if provider.is_none() {
            "AI summaries were unavailable (no active provider) — showing raw titles.".to_string()
        } else {
            "AI summaries were unavailable for some sections (provider error or budget) — showing raw titles.".to_string()
        })
    } else {
        None
    };

    let failure_warning: Option<String> = (failed_sources > FAILED_SOURCES_ALERT_THRESHOLD).then(|| {
        format!("{failed_sources} of your sources failed to fetch recently.")
    });

    let payload = json!({
        "generated_at": end_s,
        "period_start": start_s,
        "period_end": end_s,
        "ai_used": ai_used,
        "fallback_note": fallback_note,
        "failed_sources": failed_sources,
        "failure_warning": failure_warning.clone(),
        "sources": sources,
        "categories": category_json,
    });

    // 3. Archive to the user's digests row.
    let digest_id: i64 = sqlx::query(
        "INSERT INTO digests (user_id, period_start, period_end, item_count, payload_json, notified)
         VALUES (?, ?, ?, ?, ?, 0) RETURNING id",
    )
    .bind(user_id)
    .bind(start_s)
    .bind(end_s)
    .bind(item_count)
    .bind(payload.to_string())
    .fetch_one(pool)
    .await?
    .get("id");

    // 4. Push the ntfy summary if the user enabled it and has a channel (§7a). No channel → still
    //    archived, just not pushed. Failures are recorded, never fatal.
    let mut pushed = false;
    if item_count > 0 && user_wants_digest_push(pool, user_id).await {
        if let Ok(Some(ch)) = notify::resolve_channel(pool, enc_key, user_id).await {
            let body = digest_push_body(item_count, &category_counts, failure_warning.as_deref());
            let push = notify::Push {
                title: "Digestly digest".into(),
                message: body,
                tags: vec!["newspaper".into()],
                click: None,
            };
            match notify::send(http, &ch, &push).await {
                Ok(()) => {
                    pushed = true;
                    let _ = sqlx::query("UPDATE digests SET notified = 1 WHERE id = ?").bind(digest_id).execute(pool).await;
                }
                Err(e) => {
                    let _ = sqlx::query("UPDATE digests SET error = ? WHERE id = ?").bind(&e).bind(digest_id).execute(pool).await;
                    warn!(user_id, error = %e, "digest push failed");
                }
            }
        }
    }
    Ok(pushed)
}

/// One AI prompt per category (§6 digest prompt). Returns the summary text, or an error reason that
/// triggers the raw fallback (never propagates — the caller degrades gracefully).
async fn summarize_category(
    pool: &SqlitePool,
    http: &Client,
    provider: &ResolvedProvider,
    params: &AiParams,
    category: &str,
    items: &[DigestItem],
) -> Result<String, String> {
    // Budget guard before spending tokens (§6).
    budget::check(pool, params).await?;

    let mut list = String::new();
    for it in items.iter().take(MAX_ITEMS_PER_CATEGORY_PROMPT) {
        list.push_str("- ");
        list.push_str(&it.title);
        list.push_str(" (");
        list.push_str(&it.feed_title);
        list.push_str(")\n");
    }

    let system = "You are writing a section of a personal reading digest. Summarize these \
                  developments in 3-4 concise bullets, focusing on what is NEW and IMPORTANT. \
                  Output plain-text bullets each starting with '- '. Do not invent items not in \
                  the list."
        .to_string();
    let user = format!("Category: {category}\n\nHeadlines this period:\n{list}");

    let llm = client::make_client(
        http.clone(),
        provider.api_style,
        provider.base_url.clone(),
        provider.model.clone(),
        provider.key.clone(),
        params.timeout_secs,
    );
    let req = LlmRequest { system, user, max_tokens: params.max_tokens, temperature: params.temperature };
    let resp = llm.complete(&req).await.map_err(|e| e.user_message())?;
    budget::record(pool, resp.tokens_used).await;
    Ok(resp.text)
}

/// "42 new articles across AI (14), Software Engineering (12)…" (§7a).
fn digest_push_body(total: i64, counts: &[(String, i64)], warning: Option<&str>) -> String {
    let parts: Vec<String> = counts.iter().map(|(name, n)| format!("{name} ({n})")).collect();
    let mut body = if parts.is_empty() {
        format!("{total} new articles")
    } else {
        format!("{total} new articles across {}", parts.join(", "))
    };
    if let Some(w) = warning {
        body.push_str("\n⚠ ");
        body.push_str(w);
    }
    body
}

async fn failed_source_count(pool: &SqlitePool, user_id: i64) -> Result<i64> {
    Ok(sqlx::query(
        "SELECT COUNT(DISTINCT fe.id) AS n
         FROM subscriptions s JOIN feeds fe ON fe.id = s.feed_id
         WHERE s.user_id = ? AND s.disabled = 0 AND (fe.failure_count > 0 OR fe.disabled = 1)",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?
    .get("n"))
}

async fn user_wants_digest_push(pool: &SqlitePool, user_id: i64) -> bool {
    sqlx::query("SELECT notify_on_digest FROM user_notifications WHERE user_id = ?")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .map(|r| r.get::<i64, _>("notify_on_digest") != 0)
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Background scheduler
// ---------------------------------------------------------------------------

/// Idle tick for the cron check. Shorter than a minute so every scheduled minute is observed.
const SCHED_TICK_SECS: u64 = 45;

/// Spawn the digest scheduler: on each tick, if enabled and the cron matches the current minute (in
/// the configured timezone, DST-correct), run for all users — guarded to fire once per minute.
pub fn spawn(pool: SqlitePool, http: Client, enc_key: [u8; 32]) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!("digest scheduler started");
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(SCHED_TICK_SECS)).await;
            if let Err(e) = tick(&pool, &http, &enc_key).await {
                warn!(error = %e, "digest scheduler tick failed");
            }
        }
    })
}

async fn tick(pool: &SqlitePool, http: &Client, enc_key: &[u8; 32]) -> Result<()> {
    let cfg = DigestConfig::load(pool).await;
    if !cfg.enabled {
        return Ok(());
    }
    let Some(cron) = cron::Cron::parse(&cfg.cron) else { return Ok(()) };
    let tz: chrono_tz::Tz = cfg.timezone.parse().unwrap_or(chrono_tz::UTC);
    let now_local = Utc::now().with_timezone(&tz);
    if !cron.matches(&now_local) {
        return Ok(());
    }
    // Fire at most once per matching minute (ticks are sub-minute).
    let stamp = now_local.format("%Y-%m-%d %H:%M").to_string();
    if get_str(pool, "digest.last_run").await.as_deref() == Some(stamp.as_str()) {
        return Ok(());
    }
    set_setting(pool, "digest.last_run", &stamp).await?;
    info!(at = %stamp, "digest schedule fired");
    run_all(pool, http, enc_key, None).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// app_settings helpers
// ---------------------------------------------------------------------------

fn fmt_dt(dt: chrono::DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
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

async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO app_settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_filter_roundtrip_and_includes() {
        assert_eq!(CategoryFilter::from_setting("all"), CategoryFilter::All);
        assert_eq!(CategoryFilter::from_setting(""), CategoryFilter::All);
        let f = CategoryFilter::from_setting("AI, Finance");
        assert!(f.includes("ai"), "case-insensitive");
        assert!(f.includes("Finance"));
        assert!(!f.includes("Politics"));
        assert_eq!(f.to_setting(), "AI,Finance");
    }

    #[test]
    fn default_config_is_daily_at_9am_with_24h_lookback() {
        let cfg = DigestConfig::default();
        assert_eq!(cfg.cron, "0 9 * * *", "default schedule should be daily, not weekly");
        assert_eq!(cfg.lookback_days, 1, "default look-back should be 24h (1 day)");
        assert!(cron::Cron::parse(&cfg.cron).is_some(), "default cron must still be valid");
    }

    #[test]
    fn digest_push_body_lists_counts_and_warning() {
        let body = digest_push_body(26, &[("AI".into(), 14), ("Finance".into(), 12)], Some("3 of your sources failed to fetch recently."));
        assert!(body.contains("26 new articles across AI (14), Finance (12)"));
        assert!(body.contains("⚠ 3 of your sources failed"));
    }
}
