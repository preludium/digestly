//! Digest engine (prompt.md §6 "Digest", §7). The **engine is global/admin-configured** (one cron
//! schedule, look-back window, enable, categories, AI on/off) but **content is per-user**: each run
//! iterates users and builds each one a digest of *their* subscriptions grouped *by their*
//! categories, one AI prompt per non-empty category via the active provider, archived to their
//! `digests` row and pushed to their own ntfy channel (§7a).
//!
//! **AI fallback** (§6, §11): a provider error / budget exceeded produces a digest with raw grouped
//! titles + links and a note - it **never** fails the run.

pub mod cron;

use anyhow::Result;
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use reqwest::Client;
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};
use tracing::{info, warn};

use crate::ai::provider::{self, ResolvedProvider};
use crate::ai::{budget, client, AiParams, LlmRequest};
use crate::notify;
use crate::settings::{get_bool, get_int, get_str};

/// Cap on the number of items whose titles are fed to the per-category AI prompt.
const MAX_ITEMS_PER_CATEGORY_PROMPT: usize = 40;
/// A feed created without an immediate poll (OAuth sync - no backlog dump) is anchored to first
/// fetch this long before the digest's next scheduled run, so it's fresh by the time the digest
/// reads it instead of polling on an arbitrary clock offset from creation time
/// (`routes::feeds::default_next_fetch_at`).
pub const PREFETCH_BUFFER_SECS: i64 = 3600;
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
            CategoryFilter::Names(
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
            )
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
            cron: "0 5 * * *".to_string(), // daily, 05:00 UTC (§7)
            lookback_days: 1,              // 24h - lookback is stored in whole days
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
            cron: get_str(pool, "digest.cron")
                .await
                .filter(|c| cron::Cron::parse(c).is_some())
                .unwrap_or(d.cron),
            lookback_days: get_int(pool, "digest.lookback_days", d.lookback_days)
                .await
                .clamp(1, 90),
            timezone: get_str(pool, "digest.timezone").await.unwrap_or(d.timezone),
            categories: get_str(pool, "digest.categories")
                .await
                .map(|v| CategoryFilter::from_setting(&v))
                .unwrap_or(d.categories),
            ai_enabled: get_bool(pool, "digest.ai_enabled", d.ai_enabled).await,
        }
    }

    pub async fn save(&self, pool: &SqlitePool) -> Result<()> {
        set_setting(
            pool,
            "digest.enabled",
            if self.enabled { "true" } else { "false" },
        )
        .await?;
        set_setting(pool, "digest.cron", &self.cron).await?;
        set_setting(
            pool,
            "digest.lookback_days",
            &self.lookback_days.clamp(1, 90).to_string(),
        )
        .await?;
        set_setting(pool, "digest.timezone", &self.timezone).await?;
        set_setting(pool, "digest.categories", &self.categories.to_setting()).await?;
        set_setting(
            pool,
            "digest.ai_enabled",
            if self.ai_enabled { "true" } else { "false" },
        )
        .await?;
        Ok(())
    }

    /// The next UTC instant this digest is scheduled to fire, strictly after `after`, honoring
    /// `self.timezone`. `None` if `self.cron` is unparseable or can never match.
    pub fn next_run_at(&self, after: chrono::DateTime<Utc>) -> Option<chrono::DateTime<Utc>> {
        let cron = cron::Cron::parse(&self.cron)?;
        let tz: chrono_tz::Tz = self.timezone.parse().unwrap_or(chrono_tz::UTC);
        let next_local = cron.next_after(&after.with_timezone(&tz))?;
        Some(next_local.with_timezone(&Utc))
    }

    /// A human-readable schedule preview for the UI (§9.7). Notes when the engine is off.
    pub fn schedule_preview(&self) -> String {
        let base = cron::Cron::parse(&self.cron)
            .map(|c| c.describe())
            .unwrap_or_else(|| "invalid schedule".to_string());
        if self.enabled {
            format!("{base} ({})", self.timezone)
        } else {
            format!("Disabled - {base} ({})", self.timezone)
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
/// user's error - logs and continues. `lookback_override`, when `Some`, replaces the configured
/// `lookback_days` for this run only (e.g. an admin's one-off "last month" manual run) - it is
/// never persisted. The scheduled run always passes `None`.
///
/// `period_end` is one shared `Utc::now()` instant for the whole run. `period_start` is computed
/// **per user**: normally it picks up right where that user's previous digest left off (so a
/// schedule that fires more than once a day doesn't reprint the same items), falling back to the
/// full `now - lookback_days` window for a brand-new user or after a long gap. A manual
/// `lookback_override` bypasses this entirely - it always gets the explicit
/// `[now - override, now]` window, ignoring any previous digest boundary.
pub async fn run_all(
    pool: &SqlitePool,
    http: &Client,
    enc_key: &[u8; 32],
    lookback_override: Option<i64>,
) -> Result<RunSummary> {
    let cfg = DigestConfig::load(pool).await;
    let lookback_days = lookback_override
        .map(|d| d.clamp(1, 90))
        .unwrap_or(cfg.lookback_days);
    let now = Utc::now();
    let floor = now - Duration::days(lookback_days);
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

    let mut summary = RunSummary {
        users: user_ids.len(),
        ..Default::default()
    };
    for user_id in user_ids {
        // An explicit override always wins - the previous-digest boundary is only consulted
        // for the normal, un-overridden window. `start_exclusive` is true only when
        // `period_start` came from an actual previous-digest boundary (not the override, and not
        // a floor clamp) - see `build_and_archive`'s doc comment for why that's safe.
        let (period_start, start_exclusive) = if lookback_override.is_some() {
            (floor, false)
        } else {
            let last_end = last_digest_end(pool, user_id).await;
            let period_start = compute_period_start(last_end, floor);
            (period_start, period_start > floor)
        };
        let start_s = fmt_dt(period_start);

        match build_and_archive(
            pool,
            http,
            enc_key,
            user_id,
            &cfg,
            &provider,
            &params,
            &start_s,
            &end_s,
            start_exclusive,
        )
        .await
        {
            Ok(Some(pushed)) => {
                summary.digests += 1;
                if pushed {
                    summary.pushed += 1;
                }
            }
            Ok(None) => {} // no items in this user's window - nothing archived, nothing pushed
            Err(e) => warn!(user_id, error = %e, "digest build failed for user (continuing)"),
        }
    }
    info!(
        users = summary.users,
        digests = summary.digests,
        pushed = summary.pushed,
        "digest run complete"
    );
    Ok(summary)
}

/// The per-user window start: incremental from the last digest's `period_end` when it's newer
/// than `floor` (the look-back cap), else clamped to `floor` - a brand-new user or a gap longer
/// than the configured look-back both fall back to the full window.
fn compute_period_start(last_end: Option<DateTime<Utc>>, floor: DateTime<Utc>) -> DateTime<Utc> {
    last_end.map_or(floor, |e| e.max(floor))
}

/// The most recent `period_end` among this user's archived digests, or `None` if they have
/// none. `period_end` is stored as text in `fmt_dt`'s exact format (`%Y-%m-%d %H:%M:%S`), so
/// `MAX` is lexicographically correct; the result is parsed back with that same format. A NULL
/// (no rows) or unparseable value is treated as "no previous digest".
async fn last_digest_end(pool: &SqlitePool, user_id: i64) -> Option<DateTime<Utc>> {
    let raw: Option<String> =
        sqlx::query("SELECT MAX(period_end) AS last_end FROM digests WHERE user_id = ?")
            .bind(user_id)
            .fetch_one(pool)
            .await
            .ok()?
            .get("last_end");
    NaiveDateTime::parse_from_str(&raw?, "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|ndt| ndt.and_utc())
}

/// An item that belongs in a category section of the digest.
struct DigestItem {
    title: String,
    url: Option<String>,
    feed_title: String,
    published_at: Option<String>,
}

/// Build one user's digest, archive it, and (best-effort) push the ntfy summary. Returns `None`
/// if the window had no items for this user - nothing is archived and nothing is pushed - or
/// `Some(pushed)` for a created digest, `pushed` being whether the ntfy push was sent.
///
/// `start_exclusive` picks the lower-bound comparison for the item-selection window: when true
/// (the normal incremental continuation, where `start_s` is verbatim the previous digest's
/// `period_end`), the bound is `>` rather than `>=`, so an item published exactly on that
/// boundary second - already included in the previous digest via its `<= end` - isn't reprinted
/// in this one. First-ever digests, gap/floor clamps, and manual overrides keep the old
/// inclusive `>=` bound (issue #13).
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
    start_exclusive: bool,
) -> Result<Option<bool>> {
    // 1. Gather the user's in-window items grouped by their categories (ordered by category pos).
    //    The lower bound is `>` on the incremental-continuation path (see doc comment above) and
    //    `>=` everywhere else - two static query literals, never string-built from input.
    const QUERY_EXCLUSIVE_START: &str =
        "SELECT c.name AS category, c.position AS cat_pos, i.title AS title, i.url AS url,
                i.published_at AS published_at,
                COALESCE(NULLIF(s.title_override, ''), NULLIF(fe.title, ''), fe.feed_url) AS feed_title
         FROM items i
         JOIN subscriptions s ON s.feed_id = i.feed_id AND s.user_id = ?
         JOIN feeds fe ON fe.id = i.feed_id
         JOIN categories c ON c.id = s.category_id
         WHERE s.disabled = 0
           AND (s.min_score <= 0 OR (i.score IS NOT NULL AND i.score >= s.min_score))
           AND i.published_at > ? AND i.published_at <= ?
         ORDER BY c.position, c.name, i.published_at DESC";
    const QUERY_INCLUSIVE_START: &str =
        "SELECT c.name AS category, c.position AS cat_pos, i.title AS title, i.url AS url,
                i.published_at AS published_at,
                COALESCE(NULLIF(s.title_override, ''), NULLIF(fe.title, ''), fe.feed_url) AS feed_title
         FROM items i
         JOIN subscriptions s ON s.feed_id = i.feed_id AND s.user_id = ?
         JOIN feeds fe ON fe.id = i.feed_id
         JOIN categories c ON c.id = s.category_id
         WHERE s.disabled = 0
           AND (s.min_score <= 0 OR (i.score IS NOT NULL AND i.score >= s.min_score))
           AND i.published_at >= ? AND i.published_at <= ?
         ORDER BY c.position, c.name, i.published_at DESC";
    let query = if start_exclusive {
        QUERY_EXCLUSIVE_START
    } else {
        QUERY_INCLUSIVE_START
    };
    let rows = sqlx::query(query)
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
            title: r
                .get::<Option<String>, _>("title")
                .unwrap_or_else(|| "(untitled)".into()),
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
    if item_count == 0 {
        // Nothing new since last time - most often the second+ run of a same-day schedule now
        // that windows are incremental. Skip archiving a reprint-free, empty digest entirely.
        return Ok(None);
    }
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
                        warn!(user_id, category = %name, reason = %reason, "digest AI failed - raw fallback for this category");
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
            "AI summaries were unavailable (no active provider) - showing raw titles.".to_string()
        } else {
            "AI summaries were unavailable for some sections (provider error or budget) - showing raw titles.".to_string()
        })
    } else {
        None
    };

    let failure_warning: Option<String> = (failed_sources > FAILED_SOURCES_ALERT_THRESHOLD)
        .then(|| format!("{failed_sources} of your sources failed to fetch recently."));

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
    if user_wants_digest_push(pool, user_id).await {
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
                    let _ = sqlx::query("UPDATE digests SET notified = 1 WHERE id = ?")
                        .bind(digest_id)
                        .execute(pool)
                        .await;
                }
                Err(e) => {
                    let _ = sqlx::query("UPDATE digests SET error = ? WHERE id = ?")
                        .bind(&e)
                        .bind(digest_id)
                        .execute(pool)
                        .await;
                    warn!(user_id, error = %e, "digest push failed");
                }
            }
        }
    }
    Ok(Some(pushed))
}

/// One AI prompt per category (§6 digest prompt). Returns the summary text, or an error reason that
/// triggers the raw fallback (never propagates - the caller degrades gracefully).
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
    let req = LlmRequest {
        system,
        user,
        max_tokens: params.max_tokens,
        temperature: params.temperature,
    };
    let resp = llm.complete(&req).await.map_err(|e| e.user_message())?;
    budget::record(pool, resp.tokens_used).await;
    Ok(resp.text)
}

/// "42 new articles across AI (14), Software Engineering (12)…" (§7a).
fn digest_push_body(total: i64, counts: &[(String, i64)], warning: Option<&str>) -> String {
    let parts: Vec<String> = counts
        .iter()
        .map(|(name, n)| format!("{name} ({n})"))
        .collect();
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
/// the configured timezone, DST-correct), run for all users - guarded to fire once per minute.
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
    let Some(cron) = cron::Cron::parse(&cfg.cron) else {
        return Ok(());
    };
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
    fn default_config_is_daily_at_5am_utc_with_24h_lookback() {
        let cfg = DigestConfig::default();
        assert_eq!(
            cfg.cron, "0 5 * * *",
            "default schedule should be daily, not weekly"
        );
        assert_eq!(
            cfg.lookback_days, 1,
            "default look-back should be 24h (1 day)"
        );
        assert!(
            cron::Cron::parse(&cfg.cron).is_some(),
            "default cron must still be valid"
        );
    }

    #[test]
    fn digest_push_body_lists_counts_and_warning() {
        let body = digest_push_body(
            26,
            &[("AI".into(), 14), ("Finance".into(), 12)],
            Some("3 of your sources failed to fetch recently."),
        );
        assert!(body.contains("26 new articles across AI (14), Finance (12)"));
        assert!(body.contains("⚠ 3 of your sources failed"));
    }

    // -----------------------------------------------------------------------
    // Incremental per-user window (issue #13)
    // -----------------------------------------------------------------------

    #[test]
    fn compute_period_start_uses_floor_for_a_brand_new_user() {
        let floor = Utc::now() - Duration::days(1);
        assert_eq!(compute_period_start(None, floor), floor);
    }

    #[test]
    fn compute_period_start_is_incremental_when_last_digest_is_within_floor() {
        let now = Utc::now();
        let floor = now - Duration::days(1);
        let recent = now - Duration::hours(2); // newer than the floor
        assert_eq!(compute_period_start(Some(recent), floor), recent);
    }

    #[test]
    fn compute_period_start_clamps_a_stale_last_digest_to_the_floor() {
        let now = Utc::now();
        let floor = now - Duration::days(1);
        let stale = now - Duration::days(5); // older than the floor - a long gap
        assert_eq!(compute_period_start(Some(stale), floor), floor);
    }

    /// Seed a user, one category ("Other"), a feed, and a subscription tying them together -
    /// the minimum `build_and_archive`'s item-selection query needs to find items.
    async fn seed_user_with_sub(pool: &SqlitePool, username: &str) -> (i64, i64) {
        let user_id: i64 =
            sqlx::query("INSERT INTO users (username, password_hash) VALUES (?, 'hash') RETURNING id")
                .bind(username)
                .fetch_one(pool)
                .await
                .unwrap()
                .get("id");
        let category_id: i64 = sqlx::query(
            "INSERT INTO categories (user_id, name, position) VALUES (?, 'Other', 0) RETURNING id",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap()
        .get("id");
        let feed_id: i64 = sqlx::query("INSERT INTO feeds (feed_url, kind) VALUES (?, 'rss') RETURNING id")
            .bind(format!("https://feed.example/{username}.xml"))
            .fetch_one(pool)
            .await
            .unwrap()
            .get("id");
        sqlx::query("INSERT INTO subscriptions (user_id, feed_id, category_id) VALUES (?, ?, ?)")
            .bind(user_id)
            .bind(feed_id)
            .bind(category_id)
            .execute(pool)
            .await
            .unwrap();
        (user_id, feed_id)
    }

    async fn seed_item(pool: &SqlitePool, feed_id: i64, guid: &str, title: &str, published_at: &str) {
        sqlx::query(
            "INSERT INTO items (feed_id, guid, url, title, content_text, published_at, dedup_hash)
             VALUES (?, ?, ?, ?, 'body text', ?, ?)",
        )
        .bind(feed_id)
        .bind(guid)
        .bind(format!("https://ex.example/{guid}"))
        .bind(title)
        .bind(published_at)
        .bind(guid)
        .execute(pool)
        .await
        .unwrap();
    }

    /// Directly insert a digest row (bypassing `build_and_archive`) to simulate a prior run's
    /// archived boundary without needing AI/network.
    async fn seed_digest(pool: &SqlitePool, user_id: i64, period_start: &str, period_end: &str) {
        sqlx::query(
            "INSERT INTO digests (user_id, period_start, period_end, item_count, payload_json, notified)
             VALUES (?, ?, ?, 1, '{}', 0)",
        )
        .bind(user_id)
        .bind(period_start)
        .bind(period_end)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn item_count_of_latest_digest(pool: &SqlitePool, user_id: i64) -> i64 {
        sqlx::query("SELECT item_count FROM digests WHERE user_id = ? ORDER BY id DESC LIMIT 1")
            .bind(user_id)
            .fetch_one(pool)
            .await
            .unwrap()
            .get("item_count")
    }

    #[tokio::test]
    async fn last_digest_end_is_none_for_a_user_with_no_prior_digests() {
        let pool = crate::db::test_pool().await;
        let (user_id, _feed_id) = seed_user_with_sub(&pool, "alice").await;
        assert!(last_digest_end(&pool, user_id).await.is_none());
    }

    #[tokio::test]
    async fn last_digest_end_parses_the_most_recent_boundary() {
        let pool = crate::db::test_pool().await;
        let (user_id, _feed_id) = seed_user_with_sub(&pool, "alice").await;
        let older = Utc::now() - Duration::days(2);
        let newer = Utc::now() - Duration::hours(1);
        seed_digest(&pool, user_id, &fmt_dt(older - Duration::days(1)), &fmt_dt(older)).await;
        seed_digest(&pool, user_id, &fmt_dt(newer - Duration::days(1)), &fmt_dt(newer)).await;

        let last_end = last_digest_end(&pool, user_id).await.unwrap();
        assert_eq!(fmt_dt(last_end), fmt_dt(newer), "picks the MAX, not the first row");
    }

    /// (a) The headline acceptance test: a same-day second run picks up only what's new since the
    /// first run, and its window starts exactly where the first run's ended.
    #[tokio::test]
    async fn second_same_day_run_yields_disjoint_items_chained_to_the_first_runs_boundary() {
        let pool = crate::db::test_pool().await;
        let (user_id, feed_id) = seed_user_with_sub(&pool, "alice").await;

        // Simulate a "07:00" run boundary already having happened.
        let run1_end = Utc::now() - Duration::hours(8);
        let morning_item_time = fmt_dt(run1_end - Duration::hours(2)); // inside run 1's window
        seed_item(&pool, feed_id, "morning", "Morning news", &morning_item_time).await;
        seed_digest(
            &pool,
            user_id,
            &fmt_dt(run1_end - Duration::days(1)),
            &fmt_dt(run1_end),
        )
        .await;

        // An item published after the 07:00 boundary but before "now" (the simulated 15:00 run).
        let afternoon_item_time = fmt_dt(run1_end + Duration::hours(3));
        seed_item(&pool, feed_id, "afternoon", "Afternoon news", &afternoon_item_time).await;

        let http = crate::ingest::fetch::build_client();
        let enc_key = [0u8; 32];
        let summary = run_all(&pool, &http, &enc_key, None).await.unwrap();
        assert_eq!(summary.digests, 1, "one digest created for the second run");

        let row = sqlx::query(
            "SELECT period_start, item_count FROM digests WHERE user_id = ? ORDER BY id DESC LIMIT 1",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let period_start: String = row.get("period_start");
        assert_eq!(
            period_start,
            fmt_dt(run1_end),
            "second run's period_start equals the first run's period_end"
        );
        assert_eq!(
            row.get::<i64, _>("item_count"),
            1,
            "only the afternoon item is new - the morning item was already covered by run 1"
        );
    }

    /// An item published on the exact boundary second - the previous digest's `period_end` -
    /// must not reappear in the next incremental digest. It was already captured by run 1's
    /// inclusive `<= end`, so run 2 must exclude it via its exclusive `> start` lower bound
    /// (issue #13 acceptance criterion 1: no item in two consecutive digests).
    #[tokio::test]
    async fn item_published_exactly_on_the_boundary_second_is_not_reprinted() {
        let pool = crate::db::test_pool().await;
        let (user_id, feed_id) = seed_user_with_sub(&pool, "alice").await;

        let run1_end = Utc::now() - Duration::hours(8);
        seed_digest(
            &pool,
            user_id,
            &fmt_dt(run1_end - Duration::days(1)),
            &fmt_dt(run1_end),
        )
        .await;

        // Published on the exact same whole second as run 1's period_end - already included in
        // run 1 (its `<= end` captured it) - must NOT be included again in run 2.
        let boundary_time = fmt_dt(run1_end);
        seed_item(&pool, feed_id, "boundary", "Boundary news", &boundary_time).await;
        // An item strictly after the boundary, so run 2 has something to archive.
        let after_time = fmt_dt(run1_end + Duration::hours(1));
        seed_item(&pool, feed_id, "after", "After news", &after_time).await;

        let http = crate::ingest::fetch::build_client();
        let enc_key = [0u8; 32];
        let summary = run_all(&pool, &http, &enc_key, None).await.unwrap();
        assert_eq!(summary.digests, 1, "one digest created for the second run");

        let row = sqlx::query(
            "SELECT item_count, payload_json FROM digests WHERE user_id = ? ORDER BY id DESC LIMIT 1",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            row.get::<i64, _>("item_count"),
            1,
            "only the after-boundary item is new - the boundary-second item was already covered by run 1"
        );
        let payload: String = row.get("payload_json");
        assert!(
            !payload.contains("Boundary news"),
            "the boundary-second item must not be reprinted"
        );
        assert!(
            payload.contains("After news"),
            "the strictly-after item must be present"
        );
    }

    /// (b) A brand-new user (no prior digest) uses the floor `now - lookback_days`.
    #[tokio::test]
    async fn new_user_first_digest_uses_the_floor_window() {
        let pool = crate::db::test_pool().await;
        let (user_id, feed_id) = seed_user_with_sub(&pool, "alice").await;
        seed_item(&pool, feed_id, "i1", "Item", &fmt_dt(Utc::now() - Duration::hours(20))).await;

        let http = crate::ingest::fetch::build_client();
        let enc_key = [0u8; 32];
        let before = Utc::now();
        run_all(&pool, &http, &enc_key, None).await.unwrap();
        let after = Utc::now();

        let row = sqlx::query("SELECT period_start, period_end FROM digests WHERE user_id = ?")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        let start: String = row.get("period_start");
        let end: String = row.get("period_end");
        let start_dt = NaiveDateTime::parse_from_str(&start, "%Y-%m-%d %H:%M:%S")
            .unwrap()
            .and_utc();
        let end_dt = NaiveDateTime::parse_from_str(&end, "%Y-%m-%d %H:%M:%S")
            .unwrap()
            .and_utc();

        // `fmt_dt` truncates to whole seconds, so allow a 1s tolerance against `before`/`after`.
        assert!(
            end_dt >= before - Duration::seconds(1) && end_dt <= after,
            "period_end is this run's shared `now`"
        );
        assert_eq!(
            (end_dt - start_dt).num_seconds(),
            Duration::days(1).num_seconds(),
            "with no prior digest, the window is the full 1-day look-back, same as before"
        );
    }

    /// (c) A stale last digest (older than `now - lookback_days`) is clamped to the floor, not
    /// used directly - an item newer than the stale boundary but older than the floor is excluded.
    #[tokio::test]
    async fn stale_last_digest_is_clamped_to_the_floor() {
        let pool = crate::db::test_pool().await;
        let (user_id, feed_id) = seed_user_with_sub(&pool, "alice").await;

        let stale_end = Utc::now() - Duration::days(10);
        seed_digest(
            &pool,
            user_id,
            &fmt_dt(stale_end - Duration::days(1)),
            &fmt_dt(stale_end),
        )
        .await;

        // Newer than the stale boundary (would be included if it were used directly), but older
        // than the 1-day floor (must be excluded once clamped).
        seed_item(&pool, feed_id, "mid", "Mid-gap item", &fmt_dt(Utc::now() - Duration::days(5))).await;
        // Inside the floor - must be included.
        seed_item(&pool, feed_id, "recent", "Recent item", &fmt_dt(Utc::now() - Duration::hours(2))).await;

        let http = crate::ingest::fetch::build_client();
        let enc_key = [0u8; 32];
        run_all(&pool, &http, &enc_key, None).await.unwrap();

        assert_eq!(
            item_count_of_latest_digest(&pool, user_id).await,
            1,
            "only the item inside the clamped (floor) window is included"
        );
    }

    /// (d) A run that finds no new items for a user inserts no digest row and reports no digest
    /// created / no push.
    #[tokio::test]
    async fn empty_window_inserts_no_digest_row() {
        let pool = crate::db::test_pool().await;
        let (user_id, _feed_id) = seed_user_with_sub(&pool, "alice").await;

        let http = crate::ingest::fetch::build_client();
        let enc_key = [0u8; 32];
        let summary = run_all(&pool, &http, &enc_key, None).await.unwrap();
        assert_eq!(summary.digests, 0, "no digest created");
        assert_eq!(summary.pushed, 0, "nothing pushed");

        let n: i64 = sqlx::query("SELECT COUNT(*) AS n FROM digests WHERE user_id = ?")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap()
            .get("n");
        assert_eq!(n, 0, "no digest row inserted for an empty window");
    }

    /// (e) A single daily schedule (one run per day, i.e. a brand-new user every time in
    /// practice) is unchanged vs. the old behavior: window is exactly `[now - lookback_days, now]`.
    /// Covered precisely by `new_user_first_digest_uses_the_floor_window` above; this test adds
    /// the build_and_archive return-contract check for the same "created" case, plus the
    /// "skipped" case, directly.
    #[tokio::test]
    async fn build_and_archive_returns_none_when_empty_and_some_when_created() {
        let pool = crate::db::test_pool().await;
        let (user_id, feed_id) = seed_user_with_sub(&pool, "alice").await;
        let cfg = DigestConfig {
            ai_enabled: false,
            ..DigestConfig::default()
        };
        let http = crate::ingest::fetch::build_client();
        let enc_key = [0u8; 32];
        let params = AiParams::default();
        let now = Utc::now();
        let start_s = fmt_dt(now - Duration::days(1));
        let end_s = fmt_dt(now);

        let empty = build_and_archive(
            &pool, &http, &enc_key, user_id, &cfg, &None, &params, &start_s, &end_s, false,
        )
        .await
        .unwrap();
        assert!(empty.is_none(), "no items → no digest created");

        seed_item(&pool, feed_id, "i1", "Item", &fmt_dt(now - Duration::hours(2))).await;
        let created = build_and_archive(
            &pool, &http, &enc_key, user_id, &cfg, &None, &params, &start_s, &end_s, false,
        )
        .await
        .unwrap();
        assert!(created.is_some(), "with an item in range → digest created");
        assert_eq!(
            item_count_of_latest_digest(&pool, user_id).await,
            1,
            "exactly the one seeded item"
        );
    }

    /// The manual-override path ignores the previous-digest boundary entirely: the window is
    /// exactly `[now - override, now]` even when a prior (much more recent) digest exists.
    #[tokio::test]
    async fn lookback_override_ignores_the_previous_digest_boundary() {
        let pool = crate::db::test_pool().await;
        let (user_id, feed_id) = seed_user_with_sub(&pool, "alice").await;

        // A prior digest that ended 2 hours ago - if honored, an incremental run would start
        // there and miss anything older.
        let prior_end = Utc::now() - Duration::hours(2);
        seed_digest(
            &pool,
            user_id,
            &fmt_dt(prior_end - Duration::days(1)),
            &fmt_dt(prior_end),
        )
        .await;

        // Outside a 1-day incremental window (and outside the 2h-old prior boundary), but inside
        // a 7-day override.
        seed_item(&pool, feed_id, "old", "Old item", &fmt_dt(Utc::now() - Duration::days(5))).await;

        let http = crate::ingest::fetch::build_client();
        let enc_key = [0u8; 32];
        run_all(&pool, &http, &enc_key, Some(7)).await.unwrap();

        assert_eq!(
            item_count_of_latest_digest(&pool, user_id).await,
            1,
            "the 7-day override window picks up the old item, ignoring the prior boundary"
        );
    }
}
