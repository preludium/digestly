//! The background ingestion scheduler (prompt.md §4). Selects due feeds with ≥1 active
//! subscription, honors a global concurrency cap + per-host politeness, and processes each feed
//! in isolation (`Result` per feed, `tracing::warn`) so one bad feed never crashes the loop.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use reqwest::Client;
use sqlx::{Row, SqlitePool};
use tokio::sync::{Mutex as AsyncMutex, Notify, Semaphore};
use tokio::task::{JoinHandle, JoinSet};
use tracing::{debug, info, warn};

use super::fetch::{self, Conditional, FetchError, FetchOutcome};
use super::settings::IngestSettings;
use super::{parse, reddit, store, url_util, FeedKind, ParsedFeed};

/// Wake handle: notify to trigger an immediate poll (refresh-now / new subscription).
pub type IngestTrigger = Arc<Notify>;

/// Idle polling interval between scheduler ticks.
const TICK: Duration = Duration::from_secs(15);

/// Tentative lease when a feed is claimed for processing, so a slow fetch isn't re-selected.
const CLAIM_LEASE_SECS: i64 = 300;

/// Max feeds claimed per tick.
const BATCH: i64 = 50;

/// Spawn the scheduler loop. Returns a handle the caller aborts on shutdown. `enc_key` is used to
/// decrypt per-user ntfy tokens when firing feed-health notifications (§7a).
pub fn spawn(
    pool: SqlitePool,
    client: Client,
    enc_key: [u8; 32],
    trigger: IngestTrigger,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        info!("ingestion scheduler started");
        let host_locks: HostLocks = Arc::new(StdMutex::new(HashMap::new()));
        loop {
            tokio::select! {
                _ = trigger.notified() => debug!("ingestion triggered"),
                _ = tokio::time::sleep(TICK) => {}
            }
            if let Err(e) = tick(&pool, &client, &enc_key, &host_locks).await {
                warn!(error = %e, "ingestion tick failed");
            }
        }
    })
}

type HostLocks = Arc<StdMutex<HashMap<String, Arc<AsyncMutex<Option<Instant>>>>>>;

/// A feed selected for polling.
struct DueFeed {
    id: i64,
    feed_url: String,
    kind: FeedKind,
    etag: Option<String>,
    last_modified: Option<String>,
    interval: i64,
}

async fn tick(
    pool: &SqlitePool,
    client: &Client,
    enc_key: &[u8; 32],
    host_locks: &HostLocks,
) -> anyhow::Result<()> {
    let cfg = Arc::new(IngestSettings::load(pool).await);
    let due = select_due(pool).await?;
    if due.is_empty() {
        return Ok(());
    }
    debug!(count = due.len(), "polling due feeds");

    let sem = Arc::new(Semaphore::new(cfg.concurrency));
    let mut set = JoinSet::new();

    for feed in due {
        // Claim: push next_fetch_at out so this feed isn't re-selected while in flight.
        let _ = sqlx::query("UPDATE feeds SET next_fetch_at = datetime('now', ?) WHERE id = ?")
            .bind(format!("+{CLAIM_LEASE_SECS} seconds"))
            .bind(feed.id)
            .execute(pool)
            .await;

        let (pool, client, cfg, sem, host_locks, enc_key) = (
            pool.clone(),
            client.clone(),
            cfg.clone(),
            sem.clone(),
            host_locks.clone(),
            *enc_key,
        );
        set.spawn(async move {
            let _permit = sem.acquire_owned().await;
            let id = feed.id;
            if let Err(e) = process_feed(&pool, &client, &enc_key, &cfg, &host_locks, feed).await {
                // Isolated: log + record, never propagate a panic to the loop.
                warn!(feed_id = id, error = %e, "feed processing error");
            }
        });
    }
    while set.join_next().await.is_some() {}
    Ok(())
}

async fn select_due(pool: &SqlitePool) -> anyhow::Result<Vec<DueFeed>> {
    let rows = sqlx::query(
        "SELECT id, feed_url, kind, etag, last_modified, fetch_interval_secs
         FROM feeds
         WHERE disabled = 0
           AND (next_fetch_at IS NULL OR next_fetch_at <= datetime('now'))
           AND EXISTS (SELECT 1 FROM subscriptions s WHERE s.feed_id = feeds.id AND s.disabled = 0)
         ORDER BY next_fetch_at IS NOT NULL, next_fetch_at
         LIMIT ?",
    )
    .bind(BATCH)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| DueFeed {
            id: r.get("id"),
            feed_url: r.get("feed_url"),
            kind: FeedKind::from_db(r.get::<String, _>("kind").as_str()),
            etag: r.get("etag"),
            last_modified: r.get("last_modified"),
            interval: r.get("fetch_interval_secs"),
        })
        .collect())
}

/// Fetch → parse → sanitize → dedup → store one feed, updating its health state. Per-host
/// politeness is enforced by holding the host's async lock for the whole operation.
async fn process_feed(
    pool: &SqlitePool,
    client: &Client,
    enc_key: &[u8; 32],
    cfg: &IngestSettings,
    host_locks: &HostLocks,
    feed: DueFeed,
) -> anyhow::Result<()> {
    let host = url_util::host_of(&feed.feed_url);
    let lock = host_lock(host_locks, &host);
    let mut last = lock.lock().await;
    if let Some(prev) = *last {
        let elapsed = prev.elapsed();
        let delay = Duration::from_millis(cfg.per_host_delay_ms);
        if elapsed < delay {
            tokio::time::sleep(delay - elapsed).await;
        }
    }

    let result = match feed.kind {
        FeedKind::Reddit => process_reddit(pool, client, enc_key, cfg, &feed).await,
        _ => process_generic(pool, client, enc_key, cfg, &feed).await,
    };

    *last = Some(Instant::now());
    result
}

/// Fire the throttled feed-health notification when `record_failure` reported a healthy→unhealthy
/// transition (one per feed per transition, de-duped per subscriber — §7a, §11).
async fn on_failure_transition(
    pool: &SqlitePool,
    client: &Client,
    enc_key: &[u8; 32],
    feed_id: i64,
    became_unhealthy: bool,
) {
    if became_unhealthy {
        crate::notify::notify_feed_health(pool, client, enc_key, feed_id).await;
    }
}

/// RSS/Atom/JSON/YouTube path with conditional GET.
async fn process_generic(
    pool: &SqlitePool,
    client: &Client,
    enc_key: &[u8; 32],
    cfg: &IngestSettings,
    feed: &DueFeed,
) -> anyhow::Result<()> {
    let cond = Conditional {
        etag: feed.etag.as_deref(),
        last_modified: feed.last_modified.as_deref(),
    };
    match fetch::get(client, &feed.feed_url, &cond, cfg).await {
        Ok(FetchOutcome::NotModified) => {
            store::record_not_modified(pool, feed.id, feed.interval).await?;
        }
        Ok(FetchOutcome::Fetched(fetched)) => {
            let mut parsed =
                parse::parse_feed(&fetched.body, &feed.feed_url, feed.kind, cfg, Utc::now())?;
            if feed_wants_fulltext(pool, feed.id).await? {
                enrich_fulltext(client, cfg, &mut parsed.items).await;
            }
            let n = store_parsed(pool, feed.id, &parsed).await?;
            store::record_success(pool, feed.id, &fetched, feed.interval).await?;
            info!(feed_id = feed.id, new_items = n, "polled feed");
        }
        Err(e) => {
            log_failure(feed.id, &e);
            let transition = store::record_failure(pool, feed.id, &e).await?;
            on_failure_transition(pool, client, enc_key, feed.id, transition).await;
        }
    }
    Ok(())
}

/// Reddit path: JSON first (score/comments), fall back to `.rss` with NULL metrics on block.
async fn process_reddit(
    pool: &SqlitePool,
    client: &Client,
    enc_key: &[u8; 32],
    cfg: &IngestSettings,
    feed: &DueFeed,
) -> anyhow::Result<()> {
    let sub = reddit::subreddit_from_url(&feed.feed_url).unwrap_or_default();
    if sub.is_empty() {
        let e = FetchError::Disable("could not determine subreddit from URL".into());
        let transition = store::record_failure(pool, feed.id, &e).await?;
        on_failure_transition(pool, client, enc_key, feed.id, transition).await;
        return Ok(());
    }

    let no_cond = Conditional {
        etag: None,
        last_modified: None,
    };
    let json_url = reddit::json_url(&sub);
    match fetch::get(client, &json_url, &no_cond, cfg).await {
        Ok(FetchOutcome::Fetched(fetched)) => {
            let items = reddit::parse_listing(&fetched.body, cfg, Utc::now())?;
            let parsed = ParsedFeed {
                title: Some(format!("r/{sub}")),
                site_url: Some(format!("https://www.reddit.com/r/{sub}")),
                icon_url: None,
                items,
            };
            let n = store_parsed(pool, feed.id, &parsed).await?;
            store::record_success(pool, feed.id, &fetched, feed.interval).await?;
            info!(feed_id = feed.id, new_items = n, "polled reddit (json)");
            return Ok(());
        }
        Ok(FetchOutcome::NotModified) => {
            store::record_not_modified(pool, feed.id, feed.interval).await?;
            return Ok(());
        }
        Err(e) => {
            // Reddit JSON blocked/rate-limited → fall back to .rss (metrics NULL). Logged, never silent.
            warn!(feed_id = feed.id, subreddit = %sub, error = %e.message(),
                  "reddit JSON unavailable — falling back to .rss with NULL score/comments");
        }
    }

    let rss_url = reddit::rss_url(&sub);
    match fetch::get(client, &rss_url, &no_cond, cfg).await {
        Ok(FetchOutcome::Fetched(fetched)) => {
            let parsed =
                parse::parse_feed(&fetched.body, &rss_url, FeedKind::Reddit, cfg, Utc::now())?;
            let n = store_parsed(pool, feed.id, &parsed).await?;
            store::record_success(pool, feed.id, &fetched, feed.interval).await?;
            info!(
                feed_id = feed.id,
                new_items = n,
                "polled reddit (.rss fallback)"
            );
        }
        Ok(FetchOutcome::NotModified) => {
            store::record_not_modified(pool, feed.id, feed.interval).await?
        }
        Err(e) => {
            log_failure(feed.id, &e);
            let transition = store::record_failure(pool, feed.id, &e).await?;
            on_failure_transition(pool, client, enc_key, feed.id, transition).await;
        }
    }
    Ok(())
}

async fn store_parsed(
    pool: &SqlitePool,
    feed_id: i64,
    parsed: &ParsedFeed,
) -> anyhow::Result<usize> {
    store::apply_feed_metadata(pool, feed_id, parsed).await?;
    Ok(store::insert_items(pool, feed_id, &parsed.items).await?)
}

fn log_failure(feed_id: i64, e: &FetchError) {
    match e {
        FetchError::Disable(m) => warn!(feed_id, reason = %m, "disabling feed"),
        FetchError::RetryAfter(secs, m) => {
            warn!(feed_id, retry_after = secs, reason = %m, "feed rate-limited")
        }
        FetchError::Transient(m) => warn!(feed_id, reason = %m, "feed fetch failed (will backoff)"),
    }
}

/// True if any active subscription on this feed has the full-text-extract toggle on. Content is
/// shared, so the toggle is applied when *any* subscriber wants it (prompt.md §5).
async fn feed_wants_fulltext(pool: &SqlitePool, feed_id: i64) -> anyhow::Result<bool> {
    let on = sqlx::query(
        "SELECT 1 FROM subscriptions WHERE feed_id = ? AND disabled = 0 AND full_text_extract = 1 LIMIT 1",
    )
    .bind(feed_id)
    .fetch_optional(pool)
    .await?
    .is_some();
    Ok(on)
}

/// Best-effort readability enrichment for thin (summary-only) items (prompt.md §5). Fetches the
/// article and swaps in the extracted content; failure silently keeps the feed's own content.
/// Bounded per poll so a feed of long articles can't stall the loop.
async fn enrich_fulltext(client: &Client, cfg: &IngestSettings, items: &mut [super::ParsedItem]) {
    const MAX_EXTRACT: usize = 20;
    let mut done = 0;
    for item in items.iter_mut() {
        if done >= MAX_EXTRACT {
            break;
        }
        let Some(url) = item.url.clone() else {
            continue;
        };
        let thin = item.content_text.as_deref().map(|t| t.len()).unwrap_or(0) < 500;
        if !thin {
            continue;
        }
        if url_util::guard_public_url(&url, cfg.allow_private).is_err() {
            continue;
        }
        done += 1;
        let cond = Conditional {
            etag: None,
            last_modified: None,
        };
        if let Ok(FetchOutcome::Fetched(f)) = fetch::get(client, &url, &cond, cfg).await {
            let page = String::from_utf8_lossy(&f.body);
            if let Some(html) = super::content::extract_readable(&page, &url, cfg.item_content_cap)
            {
                let text = super::content::to_text(&html, cfg.item_content_cap);
                item.reading_time_secs = Some(super::content::reading_time_secs(&text));
                item.content_html = Some(html).filter(|s| !s.is_empty());
                item.content_text = Some(text).filter(|s| !s.is_empty());
            }
        }
    }
}

fn host_lock(host_locks: &HostLocks, host: &str) -> Arc<AsyncMutex<Option<Instant>>> {
    let mut map = host_locks.lock().unwrap();
    map.entry(host.to_string())
        .or_insert_with(|| Arc::new(AsyncMutex::new(None)))
        .clone()
}
