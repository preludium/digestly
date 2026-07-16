//! Background transcript fetcher (prompt.md §6a). Fetches YouTube captions for newly-ingested
//! video items shortly after they land - decoupled from the ingest scheduler tick so a slow/failed
//! transcript fetch never blocks feed polling, and throttled against `youtube.com` since caption
//! fetch shares that host with YouTube RSS polling (the ingest scheduler's per-host politeness
//! exists for exactly this reason - see `docs/youtube-feed-throttling.md`).
//!
//! Also enforces "just regular videos": Shorts are filtered at ingest (free - the channel feed
//! marks them with a distinct `/shorts/` link, see `ingest::parse::is_youtube_short`), but live
//! recordings can only be identified from YouTube's player data, which this worker already
//! fetches, so a live item is briefly ingested as normal, then deleted here the moment this
//! worker confirms `isLiveContent` (typically within seconds, since ingest wakes this worker
//! right away).

use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::Client;
use sqlx::{Row, SqlitePool};
use tokio::sync::Notify;
use tracing::{debug, info, warn};

use super::transcript;

/// Wake handle: notify to check for newly-ingested video items right away, instead of waiting for
/// the idle tick.
pub type TranscriptTrigger = Arc<Notify>;

/// Idle polling interval between worker ticks.
const TICK: Duration = Duration::from_secs(30);

/// Max items fetched per tick - kept small since each fetch is 3 requests to youtube.com.
const BATCH: i64 = 10;

/// Delay between consecutive youtube.com requests. This worker only ever talks to one host, so a
/// flat delay is enough (unlike the ingest scheduler's per-host map for many different hosts).
const HOST_DELAY: Duration = Duration::from_millis(1500);

/// Timeout for each of the (watch page / innertube / timedtext) requests within one fetch.
const FETCH_TIMEOUT_SECS: u64 = 20;

pub fn spawn(
    pool: SqlitePool,
    client: Client,
    trigger: TranscriptTrigger,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!("transcript worker started");
        let mut backoff = RateLimitBackoff::default();
        loop {
            tokio::select! {
                _ = trigger.notified() => debug!("transcript fetch triggered"),
                _ = tokio::time::sleep(TICK) => {}
            }
            if let Some(remaining) = backoff.remaining(Instant::now()) {
                debug!(
                    remaining_secs = remaining.as_secs(),
                    "still cooling down after a rate-limit hit, skipping tick"
                );
                continue;
            }
            match tick(&pool, &client).await {
                Ok(hit_rate_limit) if hit_rate_limit => backoff.record_hit(Instant::now()),
                Ok(_) => backoff.reset(),
                Err(e) => warn!(error = %e, "transcript worker tick failed"),
            }
        }
    })
}

/// Backoff after a YouTube rate-limit hit, so the worker doesn't keep hammering a host that's
/// actively throttling it — doubling per consecutive hit, capped, same shape as the ingest
/// scheduler's feed-failure backoff (`ingest::backoff_secs`). Resets on any tick that completes
/// without hitting a 429.
#[derive(Default)]
struct RateLimitBackoff {
    consecutive_hits: i64,
    until: Option<Instant>,
}

impl RateLimitBackoff {
    fn remaining(&self, now: Instant) -> Option<Duration> {
        self.until.and_then(|u| u.checked_duration_since(now))
    }

    fn record_hit(&mut self, now: Instant) {
        self.consecutive_hits += 1;
        let secs = crate::ingest::backoff_secs(self.consecutive_hits, 0.0);
        info!(
            consecutive_hits = self.consecutive_hits,
            cooldown_secs = secs,
            "backing off after rate-limit"
        );
        self.until = Some(now + Duration::from_secs(secs as u64));
    }

    fn reset(&mut self) {
        self.consecutive_hits = 0;
        self.until = None;
    }
}

struct DueItem {
    id: i64,
    url: Option<String>,
    guid: Option<String>,
}

/// Runs one batch. Returns whether a rate-limit hit occurred, so the caller can back off.
async fn tick(pool: &SqlitePool, client: &Client) -> anyhow::Result<bool> {
    let due = select_due(pool).await?;
    if due.is_empty() {
        return Ok(false);
    }
    let total = due.len();
    info!(count = total, "fetching transcripts");
    let (mut fetched, mut unavailable, mut removed_live, mut rate_limited) =
        (0u32, 0u32, 0u32, 0u32);
    for (i, item) in due.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(HOST_DELAY).await;
        }
        debug!(
            item_id = item.id,
            progress = format!("{}/{total}", i + 1),
            "fetching transcript"
        );
        let outcome = transcript::fetch_store_or_remove_if_live(
            pool,
            client,
            item.id,
            item.url.as_deref(),
            item.guid.as_deref(),
            FETCH_TIMEOUT_SECS,
        )
        .await;
        match outcome {
            transcript::TranscriptOutcome::Fetched => fetched += 1,
            transcript::TranscriptOutcome::Unavailable => unavailable += 1,
            transcript::TranscriptOutcome::RemovedLive => removed_live += 1,
            transcript::TranscriptOutcome::RateLimited => {
                rate_limited += 1;
                // A 429 is IP-wide, not per-video - the rest of this batch would almost certainly
                // hit the same wall. Stop now; everything left stays `transcript_status = 'none'`
                // and gets picked up again next tick once the limit clears.
                warn!("rate-limited by youtube.com - stopping this batch early");
                break;
            }
        }
    }
    info!(
        fetched,
        unavailable, removed_live, rate_limited, "transcript batch complete"
    );
    Ok(rate_limited > 0)
}

/// Select the oldest `BATCH` YouTube items still awaiting a transcript attempt.
async fn select_due(pool: &SqlitePool) -> anyhow::Result<Vec<DueItem>> {
    let rows = sqlx::query(
        "SELECT i.id AS id, i.url AS url, i.guid AS guid
         FROM items i
         JOIN feeds f ON f.id = i.feed_id
         WHERE f.kind = 'youtube' AND i.transcript_status = 'none'
         ORDER BY i.id
         LIMIT ?",
    )
    .bind(BATCH)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| DueItem {
            id: r.get("id"),
            url: r.get("url"),
            guid: r.get("guid"),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    async fn make_feed(pool: &SqlitePool, kind: &str) -> i64 {
        sqlx::query("INSERT INTO feeds (feed_url, kind, fetch_interval_secs) VALUES (?, ?, 3600) RETURNING id")
            .bind(format!("https://example.com/{kind}"))
            .bind(kind)
            .fetch_one(pool)
            .await
            .unwrap()
            .get("id")
    }

    async fn make_item(
        pool: &SqlitePool,
        feed_id: i64,
        dedup_hash: &str,
        transcript_status: &str,
    ) -> i64 {
        sqlx::query(
            "INSERT INTO items (feed_id, dedup_hash, transcript_status, published_at)
             VALUES (?, ?, ?, datetime('now')) RETURNING id",
        )
        .bind(feed_id)
        .bind(dedup_hash)
        .bind(transcript_status)
        .fetch_one(pool)
        .await
        .unwrap()
        .get("id")
    }

    #[tokio::test]
    async fn selects_only_youtube_items_awaiting_a_transcript() {
        let pool = test_pool().await;
        let yt_feed = make_feed(&pool, "youtube").await;
        let rss_feed = make_feed(&pool, "rss").await;

        let due = make_item(&pool, yt_feed, "a", "none").await;
        make_item(&pool, yt_feed, "b", "fetched").await; // already handled
        make_item(&pool, yt_feed, "c", "unavailable").await; // already handled
        make_item(&pool, rss_feed, "d", "none").await; // not a video, must be excluded

        let selected = select_due(&pool).await.unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id, due);
    }

    #[tokio::test]
    async fn respects_the_batch_limit_oldest_first() {
        let pool = test_pool().await;
        let yt_feed = make_feed(&pool, "youtube").await;
        let mut ids = Vec::new();
        for i in 0..(BATCH + 5) {
            ids.push(make_item(&pool, yt_feed, &format!("v{i}"), "none").await);
        }

        let selected = select_due(&pool).await.unwrap();
        assert_eq!(selected.len(), BATCH as usize);
        let selected_ids: Vec<i64> = selected.iter().map(|d| d.id).collect();
        assert_eq!(selected_ids, ids[..BATCH as usize]);
    }

    #[tokio::test]
    async fn empty_when_nothing_due() {
        let pool = test_pool().await;
        let yt_feed = make_feed(&pool, "youtube").await;
        make_item(&pool, yt_feed, "a", "fetched").await;
        assert!(select_due(&pool).await.unwrap().is_empty());
    }

    #[test]
    fn fresh_backoff_has_no_cooldown() {
        let b = RateLimitBackoff::default();
        assert_eq!(b.remaining(Instant::now()), None);
    }

    #[test]
    fn first_hit_backs_off_60_seconds() {
        let mut b = RateLimitBackoff::default();
        let t0 = Instant::now();
        b.record_hit(t0);
        assert!(b.remaining(t0).unwrap() >= Duration::from_secs(59));
        assert_eq!(b.remaining(t0 + Duration::from_secs(61)), None);
    }

    #[test]
    fn consecutive_hits_double_the_cooldown() {
        let mut b = RateLimitBackoff::default();
        let t0 = Instant::now();
        b.record_hit(t0); // 60s
        b.record_hit(t0); // escalates to 120s
        assert!(b.remaining(t0 + Duration::from_secs(100)).is_some());
        assert_eq!(b.remaining(t0 + Duration::from_secs(121)), None);
    }

    #[test]
    fn reset_clears_cooldown_and_hit_count() {
        let mut b = RateLimitBackoff::default();
        let t0 = Instant::now();
        b.record_hit(t0);
        b.reset();
        assert_eq!(b.remaining(t0), None);
        // A fresh hit after reset goes back to the base 60s, not a continued escalation.
        b.record_hit(t0);
        assert_eq!(b.remaining(t0 + Duration::from_secs(61)), None);
        assert!(b.remaining(t0 + Duration::from_secs(30)).is_some());
    }
}
