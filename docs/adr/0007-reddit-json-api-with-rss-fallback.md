# Reddit metrics fetched from the JSON/OAuth API; RSS is the fallback

**Status: accepted.**

Reddit's `.rss` feed does not include `score`, `num_comments`, or `upvote_ratio`. These fields
are what make Reddit items useful to filter and sort (the `min_score` feature depends on them).
The JSON and OAuth APIs provide them; RSS does not.

The polling tier in `backend/src/ingest/reddit.rs` and `ingest/scheduler.rs` follows a three-step
hierarchy per subreddit poll:

1. **Authenticated OAuth API** (`oauth.reddit.com/r/<sub>/top`) - used when a Reddit account is
   connected instance-wide (via `REDDIT_OAUTH_*` credentials and a connected user). Not
   subject to the public endpoint's rate-limiting. Returns score/comments/upvote_ratio reliably.
2. **Public JSON endpoint** (`reddit.com/r/<sub>/top.json`) - unauthenticated, descriptive
   `User-Agent`, `429`/`Retry-After` handling. Returns metrics but is aggressively rate-limited
   when called without auth.
3. **RSS fallback** (`reddit.com/r/<sub>/.rss`) - used only when both JSON paths fail. Metrics
   fields are stored as `NULL`. The bypass is always logged; it is never silent.

`min_score` filtering is applied at query time (in `routes/items.rs`) since items are global and
the floor is per-subscription.

## Considered options

**RSS-only.** Simpler path, no fallback logic. Rejected: loses score/comments/upvote_ratio, which
makes Reddit feeds significantly less useful (no min_score filtering, no sort by score).

**JSON-only, error on block.** Cleaner code. Rejected: a blocked JSON endpoint would disable all
Reddit polling for the instance; graceful degradation to RSS with a logged warning is the better
user outcome.

## Consequences

The application prefers authenticated Reddit API when credentials are configured; otherwise falls
back through public JSON to RSS. The fallback hierarchy is tested in isolation. RSS-backed items
have NULL metric columns, and the per-subscription `min_score` floor is a no-op for them (NULLs
are not filtered out).
