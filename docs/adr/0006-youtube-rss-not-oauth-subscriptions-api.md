# YouTube feeds polled via per-channel RSS, not the OAuth Subscriptions API

**Status: accepted.**

YouTube exposes a per-channel RSS feed at
`https://www.youtube.com/feeds/videos.xml?channel_id=<ID>` that requires no authentication and
carries no API quota. A long-running poller that refreshes feeds on a daily cadence needs neither;
the OAuth Subscriptions API exists to enumerate what a user is subscribed to, not to poll content.

Digestly resolves YouTube channel URLs, `@handle` slugs, and raw channel IDs to a `channel_id`
via `backend/src/ingest/discover.rs` (`youtube_candidate`), then stores the RSS URL as the
`feed_url`. All polling goes through the generic ingest path - no YouTube-specific API client, no
OAuth token lifecycle for polling.

## Considered options

**YouTube Data API v3 (OAuth or API key) for video listings.** Requires a Google Cloud project,
OAuth consent screen, and quota management. The free daily quota (10 000 units) is exhausted
quickly with many channels at a short interval; quota resets require waiting or paying. Rejected:
unnecessary complexity and cost for a self-hosted daily-digest reader.

**YouTube RSS with a per-user OAuth token for channel discovery at poll time.** Mixes two
concerns: discovery (one-time import) and polling (recurring). Rejected: the import step
(see `backend/src/oauth/`) handles discovery separately; polling stays token-free.

## Consequences

YouTube channel feeds are polled as plain HTTP with no authentication and no quota. The trade-off
is that YouTube rate-limits bursty unauthenticated pollers with soft-blocks (random 404/500/timeout
across many channel feeds in a short window) rather than a clean 429. See
`docs/youtube-feed-throttling.md` for the diagnosis and proposed fix (not yet implemented).

One-time OAuth import of followed channels (via `GOOGLE_OAUTH_*`) is a separate, optional
feature that runs only at sync time and does not affect polling.
