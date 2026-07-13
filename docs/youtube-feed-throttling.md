# Per-host YouTube throttling (feature to implement)

## Problem

A lot of YouTube feed polls fail at once, with a mix of `404 Not Found`, `HTTP 500`, and
`request timed out` spread across many unrelated `feed_id`s in the same short window (observed
2026-07-10, ~03:51–04:27). This is not many channels dying simultaneously - it's YouTube
soft-blocking a bursty, non-browser client.

## Root cause

`src/ingest/scheduler.rs`'s per-host politeness lock (`host_lock`) keys purely by hostname. Every
YouTube channel feed (`www.youtube.com/feeds/videos.xml?channel_id=...`) shares **one** throttle
slot, gated only by `per_host_delay_ms` (default 1500ms, `src/ingest/settings.rs`). With up to
`BATCH = 50` feeds claimed per scheduler tick (`TICK = 15s`), a batch containing several YouTube
subscriptions produces dozens of requests to `youtube.com` within under a minute - same source
IP, same static `User-Agent`, cookies off (`fetch::build_client`).

YouTube's anti-scrape defenses respond to that request pattern with inconsistent soft-blocks
(random 404/500/timeout) rather than a clean `429`/`Retry-After`. `fetch.rs::classify_status`
correctly treats `404` as `Transient` (backoff, not disable), so this doesn't currently break
anything permanently - but it can't distinguish "YouTube soft-blocked me" from "this channel is
actually gone," and it generates a burst of spurious feed-health notifications
(`on_failure_transition` → `notify::notify_feed_health`) for feeds that are actually fine.

This matches known behavior in other self-hosted RSS readers (FreshRSS, Miniflux) when polling
many YouTube channels from a single IP.

## Proposed fix

Add YouTube-specific throttling instead of relying on the generic per-host mutex + fixed delay:

1. **Larger delay for `youtube.com`** - either a higher `per_host_delay_ms` override keyed by
   host (e.g. a small `HashMap<&str, u64>` of per-host overrides, defaulting to the global
   setting), or a dedicated `ingest.youtube_delay_ms` setting.
2. **Cap YouTube feeds claimed per tick** - spread a large batch of YouTube subscriptions across
   multiple ticks instead of firing them all in one `BATCH` window, so `youtube.com` never gets
   more than N requests per minute regardless of total subscription count.
3. (Optional) Treat repeated 404 bursts across _many different_ `feed_id`s within a short window
   as a signal to back off the whole host, not just the individual feed - avoids flooding
   feed-health notifications when the cause is host-level, not per-feed.

Not yet implemented - this doc captures the diagnosis and direction so the fix can be picked up
later without re-deriving it.
