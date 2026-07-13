# Ingestion engine

The ingestion engine is the heart of Digestly. Feeds are polled **once** for all users by a background `tokio` scheduler; one bad feed never crashes the loop.

**Source:** `src/ingest/` (scheduler, fetch, parse, store, reddit, content, discover, settings, url_util)

## Scheduler loop (`src/ingest/scheduler.rs`)

A `tokio` background task that runs the ingestion loop:

1. **Select due feeds.** `next_fetch_at <= now`, not `disabled`, and having **≥1 active subscription** (orphan feeds are skipped). A claim-lease prevents a slow fetch from being re-selected. Respects a global concurrency cap (semaphore) and per-host politeness (one in-flight request per hostname + a minimum delay).
2. **Conditional GET.** Sends stored `ETag` / `If-Modified-Since`; on `304` it just updates `last_fetch_at` and reschedules. `Accept-Encoding: gzip, brotli`, a real `User-Agent`, and a per-request timeout (default 20s).
3. **Parse → sanitize → dedup → store.** `feed-rs` parses RSS/Atom/JSON uniformly; HTML is sanitized with `ammonia`; relative URLs are rewritten to absolute; a lead image and reading time are extracted; new items are inserted in a transaction (FTS kept in sync by triggers).
4. **Success/failure bookkeeping.** On success: reset `failure_count`, clear `last_error`, set `next_fetch_at`. On failure: increment `failure_count`, store `last_error`, reschedule with **exponential backoff + jitter** (capped ~6h). After repeated failures or a terminal status the feed is disabled.

**Per-feed isolation:** each fetch is a `Result` handled independently with `tracing::warn`.

The scheduler is woken by an `IngestTrigger` (`tokio::sync::Notify`) when the API adds a new subscription or the user clicks "Refresh all feeds." The trigger is held in `AppState` and passed to the scheduler at boot.

## Fetch pipeline (`src/ingest/fetch.rs`)

- Uses `reqwest` with `rustls`, gzip/brotli, cookies off
- Per-request timeout
- Conditional GET via `ETag`/`If-Modified-Since`
- Follows `301`/`308` redirects and persists the new `feed_url`
- Honors `429`/`Retry-After` with backoff
- Returns the raw response bytes + etag/last_modified for persisting

## Parse pipeline (`src/ingest/parse.rs`)

- Uses `feed-rs` which handles RSS 2.0, RSS 1.0-RDF, Atom, and JSON Feed uniformly
- Extracts: title, site_url, icon_url, per-item guid, url, title, author, content, published date
- Returns a `ParsedFeed` with `Vec<ParsedItem>`

## Content handling (`src/ingest/content.rs`)

- HTML sanitization via `ammonia` (scripts, handlers, `javascript:` URLs stripped)
- Rewrites relative URLs to absolute (base = item link)
- Extracts lead image: `media:content`, `enclosure`, OG `og:image`, YouTube thumbnail, or first `<img>`
- Computes `reading_time_secs` (~200 wpm) from content text
- Stores both `content_html` (sanitized) and `content_text` (stripped, for FTS + AI)

## Store (`src/ingest/store.rs`)

- Deduplication: prefers GUID, then normalized URL, then hash of title+content (`dedup_key` in `mod.rs`)
- Inserts only new items in a transaction
- FTS5 sync handled by insert/update/delete triggers
- Success/failure: resets or increments `failure_count`, updates `next_fetch_at` with backoff
- Terminal statuses: `410 Gone` → auto-disable; `401`/`403` → auto-disable; `404` → back off then disable after repeated failures

## Reddit specifics (`src/ingest/reddit.rs`)

The `.rss` feed lacks `score`/`comments`, so Digestly uses the JSON endpoint (`.../top.json`) for `score`, `num_comments`, `upvote_ratio` with a descriptive `User-Agent` and `429`/`Retry-After` handling.

If JSON is blocked/unavailable it **falls back to `.rss`** with those metrics stored as `NULL` and logs the bypass. With NULL score, the per-feed `min_score` filter is skipped for those items. `min_score` is applied at **query time** since items are shared.

## YouTube specifics

- Polled via **per-channel RSS** (`https://www.youtube.com/feeds/videos.xml?channel_id=<ID>`). No OAuth, no quota.
- Handles/`@handle`/channel URLs are resolved to a `channel_id` via page scraping (`src/ingest/discover.rs`).
- **Transcript fetch:** for each new video, fetches captions (YouTube `timedtext`/caption track; prefers manual, falls back to auto-generated). Stored in `transcript_text` with `transcript_status ∈ {fetched, unavailable}`. Done lazily/async by a background transcript worker so a slow transcript never blocks other feeds. If no captions → `transcript_status = unavailable`, falls back to description.
- The scheduler notifies the transcript worker via `new_video_trigger` (`TranscriptTrigger`) when new YouTube items are stored.

## Discovery (`src/ingest/discover.rs`)

- Given a URL, discovers the feed URL and metadata
- Handles YouTube URLs, Reddit subreddit URLs, and generic feed auto-discovery
- Returns a `DiscoverCandidate` with feed_url, title, kind, site_url, icon_url, and whether already subscribed

## Feed settings (`src/ingest/settings.rs`)

Global ingestion tunables stored in `app_settings` (admin-only):
- `ingest.fetch_interval_secs` — default poll interval
- `ingest.concurrency` — global concurrency cap
- `ingest.per_host_delay_ms` — politeness delay per hostname
- `ingest.request_timeout_secs` — per-request timeout
- `ingest.allow_private_ips` — SSRF guard toggle

## URL utilities (`src/ingest/url_util.rs`)

- Normalizes feed URLs for deduplication (strip trailing slashes, lowercase host, etc.)
- Validates and canonicalizes URLs
