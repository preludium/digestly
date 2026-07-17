# Feeds are fetched once for the instance; only state is per-user

**Status: accepted.**

A [[Feed]] that ten users subscribe to is polled once per cycle, not ten times. The fetched
content (feeds, items, transcripts, AI summaries) lives in global tables with no `user_id`. Only
state - what the user has read, starred, subscribed to, categorized, and received in digests -
carries a `user_id` and is cascade-deleted with the account.

The schema split from `backend/migrations/0001_init.sql`:

| Global (shared, no `user_id`)                                                   | Per-user (`user_id`, cascade-deleted with account)                                         |
| ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `feeds`, `items`, `items_fts`, `item_summaries`, `ai_providers`, `app_settings` | `categories`, `subscriptions`, `item_states`, `settings`, `user_notifications`, `digests` |

**Shared summary cache.** `item_summaries` is keyed by `(item_id, model)` and is not per-user.
Two users requesting an AI summary of the same item with the same model get the cached result;
the provider is called once. This is a direct consequence of shared content: since the item is
global, its summary can be too.

**Scoping is enforced server-side.** The `CurrentUser` extractor derives `user_id` from the
signed session cookie. It is never a client-supplied parameter. No endpoint reads or mutates
another user's rows except the admin user-management endpoints, which manage accounts only.

**Related decision - per-user digest window.** The digest engine shares this content axis but has
an additional per-user dimension: which window of items to include in each user's digest. See
ADR-0004 (`docs/adr/0004-per-user-incremental-digest-window.md`) for how the digest window is
computed per user from that user's last digest boundary, using the shared `items` table as the
source. Shared ingest and shared summaries are the axis this ADR governs; per-user digest windows
are a distinct but related axis.

## Considered options

**Per-user ingest (each user's subscriptions polled separately).** Simpler data model (no
global/per-user split). Rejected: a feed with N subscribers gets polled N times; at modest user
counts this wastes bandwidth and exhausts per-host rate limits. The shared summary cache also
becomes impossible - the same item would exist N times under N different user IDs.

**Per-user items table, global feeds table.** Hybrid: items duplicated per subscriber. Rejected:
item content is identical across users; storing it N times wastes space and complicates any
cross-user query (e.g. "which items exist, regardless of user?").

## Consequences

A user's subscription points to a row in the global `feeds` catalog (via `subscriptions.feed_id`).
The user's read/star state is a separate row in `item_states` (absent = unread/unstarred). Orphan
feeds (feeds with no active subscriptions) are skipped by the [[Ingest Scheduler]] and may be
garbage-collected.
