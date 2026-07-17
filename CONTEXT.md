# Digestly domain glossary

Terms used in code, docs, and issues. Each term is defined once here. If you see a term used
differently in two places, that is a discrepancy - update the usage, not this file.

## Flagged ambiguities

**"Source" is overloaded.** In digest output (`docs/example-digest.md`) "Sources" means the feed
titles listed under a digest section. In general prose "source" often means the originating URL
or platform (YouTube, Reddit, RSS). When precision matters, say "[[Feed]]" or "feed title",
not "source".

**"Ingest" is a verb and a module.** "The ingest ran" (verb: the polling cycle completed). "The
`ingest` module" (code: `backend/src/ingest/`). These are the same concept; context
disambiguates. Avoid "ingestion" and "ingest" in the same sentence for the same thing.

---

## Terms

**[[Feed]]** - a single subscription to a content stream. Stored in the `feeds` table (shared
across all users - one row per URL, fetched once for the instance). A feed has a `kind`:
`rss`, `atom`, `jsonfeed`, `youtube`, or `reddit`. A user subscribes to a feed via the
`user_feeds` table; the subscription carries the category, read/star state, fetch interval, and
display preferences. _Avoid:_ "channel" (YouTube-specific; use Feed universally).

**[[Item]]** - a single content entry from a [[Feed]]: an article, a video, or a post. Stored in
the `items` table (global, not per-user). Per-user read/star state lives in `item_states`. AI
summaries (on-demand, keyed by `(item_id, model)`) live in `item_summaries`.

**[[Digest]]** - a per-user scheduled summary of recent [[Item]]s, grouped by [[Category]]. Built
by the digest engine, archived to the `digests` table, and (optionally) pushed to the user's
[[ntfy]] channel. Content is always per-user even though the schedule and config are admin-owned
and instance-wide. See `docs/adr/0004-per-user-incremental-digest-window.md`.

**[[Ingest]]** - the background process that polls [[Feed]]s, parses new [[Item]]s, and stores
them. Implemented in `backend/src/ingest/`. The main background task is the
[[Ingest Scheduler]].

**[[Ingest Scheduler]]** - the long-lived tokio task in `backend/src/ingest/scheduler.rs` that
drives feed polling. Wakes every 15 s (`TICK`) or immediately on an `IngestTrigger` signal from
an API handler (refresh-now, new subscription). Claims feeds with a 300 s lease
(`CLAIM_LEASE_SECS`) before processing them, so a slow fetch is not re-selected by the next tick.
Processes up to 50 feeds per tick (`BATCH`). Spawned in `backend/src/main.rs:95`.

**[[Claim]]** (also: lease) - when the [[Ingest Scheduler]] selects a [[Feed]] for polling, it
pushes `next_fetch_at` forward by `CLAIM_LEASE_SECS` (300 s). This prevents the next tick from
picking up the same feed while it is in flight. The claim is overwritten by the actual
`next_fetch_at` once the poll completes.

**[[Transcript]]** - the full text of a YouTube video's captions, fetched by the transcript
worker (`backend/src/ai/transcript_worker.rs`) and stored in `items.transcript_text`. The worker
is woken by an `Arc<Notify>` (`new_video_trigger`) whenever the [[Ingest Scheduler]] stores new
YouTube items, so transcripts arrive shortly after ingest rather than on the worker's own 30 s
idle tick.

**[[Summary]]** (also: summarization) - an AI-generated text summary of a single [[Item]],
produced on demand (user presses "Summarize") via the active AI provider and stored in
`item_summaries`, keyed by `(item_id, model)`. Shared across users: two users requesting a
summary of the same item with the same model get the same cached text. Distinct from a
[[Digest]] (which summarizes many items across a time window).

**[[Category]]** - the user-defined grouping for [[Feed]] subscriptions. Every subscription
belongs to exactly one, mandatory category. Categories are also the sections of a [[Digest]].
Each account is seeded with six categories on creation (AI, Software Engineering, Finance,
Politics, Lifestyle, Other). `Other` is non-deletable; deleting any other category moves its
feeds there.

**[[Feed health]]** - the polling status of a [[Feed]]. A feed transitions from healthy
(`failure_count = 0`) to failing when enough consecutive polls fail. The
[[Ingest Scheduler]] fires a throttled [[ntfy]] notification to each subscribed user exactly once
per healthy-to-failing transition (`on_failure_transition` in `ingest/store.rs`). A feed that
remains unhealthy long enough is disabled. Feed health is visible in the app's Health screen.

**[[Soft-block]]** - YouTube's non-standard throttling response to bursty polling from a single
IP: random `404`/`500`/timeout across many unrelated feed IDs in a short window, rather than a
clean `429`/`Retry-After`. The [[Ingest Scheduler]] cannot distinguish a soft-block from a dead
channel. See `docs/youtube-feed-throttling.md` for diagnosis and the proposed fix (not yet
implemented).

**[[ntfy]]** - the push notification protocol and app used for per-user notifications. Each user
configures their own ntfy server, topic, and auth token in Settings. Digestly uses ntfy for two
event types: digest completion (if the user enabled it) and [[Feed health]] transitions. ntfy
secrets are encrypted at rest with a `SECRET_KEY`-derived key. The server is never bundled or
assumed - users bring their own.
