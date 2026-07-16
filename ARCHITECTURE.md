# Architecture

Digestly is one Rust binary that serves an embedded React SPA, backed by a single SQLite file.
This document maps the modules, the ingestion and digest flows, the data model, and the key
design decisions so an engineer can navigate the code.

## Single-service model

There is exactly one deployable service on one port with one database file:

- The Rust binary (`digestly`) serves the built SPA via `tower-http::ServeDir` with an SPA
  fallback: a real static file is served when it exists, otherwise `index.html` is returned
  with `200` so React Router deep links survive refresh/back. Unknown `/api/*` paths return a
  `404` JSON, never the SPA shell. See `backend/src/http.rs` (`spa_fallback`).
- `/api/*` is matched first (nested router); everything else falls through to the SPA.
- Storage is a single SQLite file at `${DATA_DIR}/digestly.db` in WAL mode with FTS5.
- The Docker image is a three-stage build (node build → rust build → `debian-slim` runtime)
  and runs on ARM64 and x86-64.

`backend/src/main.rs` wires it together at boot: load + validate config, create `DATA_DIR`, connect
and migrate the DB, bootstrap the built-in admin, derive keys from `SECRET_KEY`, spawn the
four background tasks (ingestion scheduler, transcript worker, digest scheduler, maintenance),
and serve with graceful shutdown on SIGTERM.

The transcript worker (`backend/src/ai/transcript_worker.rs`) fetches YouTube captions for newly-ingested
video items out of band, so a slow caption fetch never holds up feed polling. The ingestion
scheduler wakes it (a `tokio::sync::Notify`) whenever a YouTube feed poll stores new items;
otherwise it ticks on its own. It is also where live recordings are dropped from the library -
`isLiveContent` is only visible in YouTube's player data, which this worker already fetches.

## Module layout (`backend/src/`)

| Module                  | Responsibility                                                                                                                                     |
| ----------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `config.rs`             | Bootstrap env parsing + fail-fast validation (`SECRET_KEY`, `ADMIN_PASSWORD`, …).                                                                  |
| `db.rs`                 | SQLite pool (WAL, `busy_timeout`, `foreign_keys`), migrations, health ping.                                                                        |
| `http.rs`               | Router, `AppState`, CORS (Tauri origins), compression, static SPA serving, `/api/health`.                                                          |
| `error.rs`              | `AppError` / `ApiResult` - JSON error responses.                                                                                                   |
| `healthcheck.rs`        | `--healthcheck` subcommand (dependency-free TCP probe for the Docker HEALTHCHECK).                                                                 |
| `auth/`                 | argon2 passwords, passkeys/WebAuthn (`passkey.rs`), admin bootstrap, signed-cookie sessions, `CurrentUser`/`AdminUser` extractors.                 |
| `oauth/`                | OAuth import helpers (YouTube/Reddit, S4): authorize/token/refresh, subscription listing, and the idempotent `reconcile` that adds only new feeds. |
| `ingest/`               | Discovery, scheduler, fetch, parse, store, content handling, Reddit/YouTube specifics.                                                             |
| `ai/`                   | `LlmClient` trait + two clients, provider management, key crypto, summarize, budget, YouTube captions (`transcript.rs`) + the background worker that fetches them (`transcript_worker.rs`). |
| `notify.rs` / `notify/` | Per-user ntfy config + sending (digest push, throttled feed-health push).                                                                          |
| `digest/`               | Digest engine (`mod.rs`) + restricted cron parser (`cron.rs`).                                                                                     |
| `maintenance.rs`        | Periodic retention purge (starred kept forever).                                                                                                   |
| `opml.rs`               | OPML import/export round-trip (`roxmltree` for parsing).                                                                                           |
| `routes/`               | REST handlers per resource (auth, me, admin, categories, feeds, items, ai, notifications, digest, settings, opml).                                 |
| `query.rs`              | Timezone-aware `when` ranges (DST-correct) and sort/filter SQL used by the items API.                                                              |
| `seed.rs`               | Seeds the six default categories per account.                                                                                                      |
| `seed_demo.rs`          | `--seed` test-mode command: ingest fixtures offline + print a sample digest.                                                                       |

## Ingestion flow

Ingestion is global and shared - feeds are polled once for all users. A `tokio` background
scheduler (`backend/src/ingest/scheduler.rs`) runs the loop:

1. **Select due feeds.** `next_fetch_at <= now`, not `disabled`, and having **≥1 active
   subscription** (orphan feeds are skipped, and may be GC'd). A claim-lease prevents a slow
   fetch from being re-selected. Respects a global concurrency cap (semaphore) and per-host
   politeness (one in-flight request per hostname + a minimum delay).
2. **Conditional GET.** Sends stored `ETag` / `If-Modified-Since`; on `304` it just updates
   `last_fetch_at` and reschedules. `Accept-Encoding: gzip, brotli`, a real `User-Agent`, and
   a per-request timeout.
3. **Parse → sanitize → dedup → store.** `feed-rs` parses RSS/Atom/JSON uniformly; HTML is
   sanitized with `ammonia` (scripts, handlers, `javascript:` URLs stripped) before storing;
   relative URLs are rewritten to absolute; a lead image and reading time are extracted; new
   items are inserted in a transaction (FTS kept in sync by triggers).
4. **Success/failure bookkeeping.** On success: reset `failure_count`, clear `last_error`,
   set the next fetch time. On failure: increment `failure_count`, store `last_error`, and
   reschedule with **exponential backoff + jitter** (capped ~6h). After repeated failures (or
   a terminal status like `401/403/410`) the feed is disabled and surfaced in `/health`.

**Per-feed isolation:** each feed fetch is a `Result` handled independently with
`tracing::warn` - one bad feed never crashes the loop.

**Source specifics** (`backend/src/ingest/`):

- **Reddit:** the `.rss` feed lacks `score`/`comments`, so Digestly uses the JSON endpoint
  (`.../top.json`) for `score`, `num_comments`, `upvote_ratio` with a descriptive
  `User-Agent` and `429`/`Retry-After` handling. If JSON is blocked/unavailable it falls back
  to `.rss` with those metrics `NULL` (and logs the bypass - never silent). `min_score` is
  applied at **query time** since items are shared.
- **YouTube:** polled via **per-channel RSS** (no OAuth, no quota); handles/`@handle`/channel
  URLs are resolved to a `channel_id`. Transcripts are fetched lazily/async so a slow
  transcript never blocks other feeds; `transcript_status` becomes `fetched` or `unavailable`.

## Shared-ingest / per-user-state model

The schema splits cleanly into **global** (shared, no `user_id`) and **per-user** tables:

| Global (shared)                                                                 | Per-user (`user_id`, cascade-deleted)                                                     |
| ------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `feeds`, `items`, `items_fts`, `item_summaries`, `ai_providers`, `app_settings` | `categories`, `subscriptions`, `item_states`, `settings`, `user_notifications`, `digests` |

A "feed" in the UI is a `subscriptions` row (a user's link to a global `feeds` catalog entry
with _their_ category and settings). The fetched **content** (feeds + items) is shared; all
**state** (read/star, categories, min-score, notifications, digests) is per-user.

**Scoping is enforced server-side.** Every non-admin query derives `user_id` from the session
(the `CurrentUser` extractor) - it is **never** a client-supplied parameter. No endpoint reads
or mutates another user's rows, except the admin user-management endpoints (which manage
accounts only, never feed contents). `AdminUser` rejects non-admins with `403` at the server.

### Schema (`backend/migrations/0001_init.sql`)

**Accounts & auth (global):**

- `users(id, username UNIQUE, password_hash, role, disabled, created_at, last_login_at)` -
  `role ∈ {admin, user}`.
- `passkeys(...)` - WebAuthn credentials (one per authenticator; `public_key` holds the
  serialized `webauthn-rs` credential, `sign_count` mirrors the counter for the clone check).
- `sessions(id, user_id, created_at, expires_at)` - revocable server-side sessions.

**Global content:**

- `feeds(...)` - canonical catalog deduped by normalized `feed_url`; `kind ∈ {rss, atom,
jsonfeed, youtube, reddit}`; carries `etag`, `last_modified`, `next_fetch_at`,
  `fetch_interval_secs`, `failure_count`, `last_error`, `disabled`.
- `items(...)` - per feed; content_html (sanitized) + content_text, transcript + status,
  image/duration/reading-time, published/fetched, Reddit `score`/`comments_count`/
  `upvote_ratio` (nullable), `dedup_hash`.
- `items_fts` - FTS5 over `title, content_text, author`, synced by insert/update/delete
  triggers.
- `item_summaries(id, item_id, model, api_style, summary_text, created_at, UNIQUE(item_id,
model))` - the shared AI summary cache; contains no user-identifying data.
- `ai_providers(...)` - admin-managed LLM endpoints; `api_key_enc` encrypted, never returned;
  exactly one `is_active`.
- `app_settings(key, value)` - global admin config (registration toggle, ingestion tunables,
  digest engine config, AI params, retention).

**Per-user state:**

- `categories(id, user_id, name, position, ..., UNIQUE(user_id, name))` - the single grouping
  concept; `Other` is the non-deletable catch-all.
- `subscriptions(id, user_id, feed_id, category_id NOT NULL, content_type, min_score,
full_text_extract, disabled, title_override, ..., UNIQUE(user_id, feed_id))`.
- `item_states(user_id, item_id, is_read, is_starred, read_at, PK(user_id, item_id))` -
  absence = unread & unstarred; upserted on first interaction.
- `settings(user_id, key, value, PK(user_id, key))` - per-user preferences only.
- `user_notifications(user_id PK, ntfy_server_url, ntfy_topic, ntfy_auth_token_enc,
ntfy_priority, notify_on_digest, notify_on_feed_health, ...)` - encrypted token, never
  returned.
- `digests(id, user_id, created_at, period_start, period_end, item_count, payload_json,
notified, error)` - per-user digest history.

Indexes cover the hot paths: `items(feed_id, published_at)`, `items(dedup_hash)`,
`items(published_at)`, `items(score)`, `items(comments_count)`, `feeds(next_fetch_at)`, the
subscription/category/state/summary lookups.

## Authentication

- **Passwords** are hashed with argon2 (`backend/src/auth/password.rs`); plaintext is never stored or
  logged.
- **Sessions** are a signed cookie plus a revocable `sessions` table. The cookie signing key
  is derived from `SECRET_KEY` (SHA-512 → 64-byte `cookie::Key`, `SignedCookieJar`), so
  sessions survive restarts but revoke on logout / logout-all / user-delete.
- **Roles** are `admin | user`; extractors gate access (`CurrentUser`, `AdminUser`).
- **Admin bootstrap** (`backend/src/auth/bootstrap.rs`): on every boot, ensure the `admin` user exists
  with the `ADMIN_PASSWORD` hash (re-synced if the env value changed). The built-in admin
  cannot be deleted or demoted, and the instance always keeps at least one admin.
- Login errors are generic (no username enumeration).
- **OAuth import (S4, `backend/src/oauth/`).** Optional per-user linking of YouTube/Reddit to import
  followed channels/subreddits as RSS feeds. Client credentials are instance-level env
  (`GOOGLE_OAUTH_*` / `REDDIT_OAUTH_*`); a provider's feature is hidden unless both are set. The
  authorization-code flow stores only an **encrypted refresh token** per user (`user_oauth` table,
  migration `0002`), never returned or logged; the CSRF `state` binds the callback to the
  initiating user (in-process store). "Sync now" is repeatable and idempotent - it refreshes an
  access token, lists subscriptions, maps each to the same feed URL the poller uses, and calls
  `reconcile`, which reuses `feeds::subscribe_url` to add only feeds the user doesn't already have.
  Polling itself is always plain RSS/JSON; the token is used only at sync time. The mapping +
  reconcile + state logic are unit/integration tested; the provider network calls need live
  credentials.
- **Passkeys / WebAuthn** (`backend/src/auth/passkey.rs`, `backend/src/routes/passkeys.rs`): Digestly is the
  Relying Party (`webauthn-rs`), built once at boot from `RP_ID`/`RP_ORIGIN` and held in
  `AppState` (`Option`, so bad config disables the feature without blocking boot). Passwords and
  passkeys are both valid sign-in methods for the same account; the ceremony state between
  `options` and `verify` lives in a short-lived, in-process `CeremonyStore` (never persisted).
  Only the resulting `Passkey` credential is serialized into `passkeys.public_key`. Two guards
  are enforced explicitly (and mirrored by the library): **sign-count regression** rejects a
  credential whose signature counter stalls or goes backwards (cloned authenticator), and the
  **last-sign-in-method guard** refuses to delete a user's only credential when they have no
  password. Because passkeys bind to `RP_ID`, changing the hostname invalidates them.

## Pluggable AI

AI is provider-agnostic and admin-global (`backend/src/ai/`):

- **`LlmClient` trait, two implementations only** (`backend/src/ai/client.rs`): `OpenAICompatibleClient`
  (`POST {base_url}/chat/completions`, covers Groq/OpenAI/Gemini/Mistral/Ollama/most custom)
  and `AnthropicClient` (`POST {base_url}/messages`), selected by `api_style`. There is no
  provider-specific code beyond these two clients.
- **Presets** (Groq / OpenAI / Anthropic / Gemini / Mistral / Ollama) bake in base URL +
  API style; the admin supplies only a key and model. Exposed via `GET /api/ai/presets`.
- **Write-only keys.** Keys are encrypted at rest with a `SECRET_KEY`-derived
  ChaCha20-Poly1305 key (`backend/src/ai/crypto.rs`; key = SHA-256 of `SECRET_KEY`, blob =
  `nonce(12) ‖ ciphertext+tag`). Keys are never returned by any endpoint or logged; rotation
  is delete + recreate.
- **Shared summary cache.** Summaries are written to `item_summaries` keyed by `(item, model)`
  and reused across users, so the same item is never re-summarized (unless the model differs
  or the user forces a refresh).
- **SSRF guard** on custom base URLs rejects private/loopback ranges unless allow-private is
  enabled - but intentionally **allows localhost for Ollama** (`provider_type == ollama`).
- **Token budget guard** (`backend/src/ai/budget.rs`): daily/monthly token budgets checked before a
  call and recorded after; huge source lists are truncated.

## Video → readable

Video items are rendered as text, not a player (`backend/src/ai/transcript.rs`, `summarize.rs`). The
transcript is fetched by the background transcript worker shortly after ingest (and lazily, on
first summarize, if the worker hasn't got to it); the active provider produces a structured
readable summary (intro + key-point bullets + takeaways), cached in `item_summaries`. When
`transcript_status = unavailable`, the description is summarized and labelled as such, and the
watch link is surfaced more prominently. The reader renders: AI summary (primary) →
collapsible full transcript → de-emphasized "Watch on YouTube".

**Video-URL path (optional).** An admin can point `ai.video_provider_id` at a **Gemini** provider
(Settings → AI → YouTube video summaries). Gemini is then sent the video URL itself, so videos
with no captions at all still get a real summary; any failure falls back to the transcript flow.
Gemini-only - it's the only supported API that accepts a video URL as model input - and it bills
video at roughly 100 tokens per second of runtime, so the token budgets drain much faster.

**Just regular videos.** Shorts are filtered at ingest (the channel feed marks them with a
`/shorts/` link). Live recordings can only be identified from YouTube's player data, so they are
ingested normally and then deleted by the transcript worker once it confirms `isLiveContent`.

## Digest engine

`backend/src/digest/mod.rs`: a global/admin cron config (in `app_settings`) drives per-user digests.

- `DigestConfig` holds `enabled`, `cron`, `lookback_days`, `timezone`, `categories`,
  `ai_enabled`. The cron is a restricted 5-field parser (`backend/src/digest/cron.rs`) matched against
  wall-clock time in the configured timezone (DST-correct), with a `describe()` human preview.
- `run_all` resolves the active provider + params once, then iterates users. For each user it
  gathers in-window items grouped by their categories (respecting `min_score`), and produces
  **one AI prompt per non-empty category**.
- **Raw-titles fallback:** if there is no active provider, or a provider call fails, or the
  budget is exceeded, that section falls back to raw grouped titles + links with a
  `fallback_note`. The run never fails.
- Each digest is archived to the user's `digests` row as a structured `payload_json`
  (categories, sources, `ai_used`, `fallback_note`, `failure_warning`) and, if the user
  enabled digest pushes and has a channel, pushed to their ntfy.
- If more than two of a user's sources failed to fetch in the window, a `failure_warning` is
  included in both the digest and the push.

The scheduler ticks every 45s and fires at most once per matching minute (a `digest.last_run`
stamp guard).

## ntfy notifications

Per-user (`backend/src/notify`): config lives in `user_notifications` (server URL, topic, encrypted
write-only token, priority, per-event toggles). Sending is a plain HTTP `POST {server}/{topic}`
with Title/Priority/Tags (+ auth) headers, a 10s timeout and one retry; failures are logged and
surfaced, never fatal. The SSRF guard **allows** the user-configured ntfy host (often
localhost/LAN) while validating the URL. Feed-health pushes are throttled to one per feed per
healthy→failing/disabled transition and de-duped per subscriber, so a feed shared by many users
notifies each at most once per transition.

## Retention / maintenance

`backend/src/maintenance.rs` runs a periodic purge (every 6h) driven by `retention.max_age_days` and
`retention.max_per_feed` in `app_settings` (both `0` = keep forever). **Starred items are
never purged** - an item starred by _any_ user survives. Deletes cascade to
`item_states`/`item_summaries` and keep FTS in sync via the delete trigger.

## Frontend

One React app (`web/`) powers the browser, the installed PWA (and, in future, Tauri):

- **TanStack Query** for all server state (reads and mutations via hooks).
- **Zustand** for ephemeral UI state (drawer, theme, toasts).
- **shadcn-style components** on **Tailwind** with design tokens (no raw hex / arbitrary
  values in components).
- **React Router** for client-side routing; the SPA fallback on the server makes deep links
  work.
- **URL as source of truth** for feed filters (`?type=&status=&when=&cat=&sort=&page=`) so
  state survives refresh/back and is shareable.
- **PWA:** `web/public/manifest.webmanifest` + `web/public/sw.js` (registered via
  `web/src/lib/pwa.ts`) provide app-shell caching, installability, and offline reading of
  already-fetched items.
- **Offline write-sync (S3):** read/star mutations made offline are applied optimistically to the
  query cache and appended to a persistent **outbox** (`web/src/lib/outbox.ts`, localStorage-backed;
  wired to the network + React in `web/src/lib/sync.ts`). Each entry carries an _explicit_ value;
  before replay the outbox coalesces per `(kind,item)` to the latest intent, so replay is idempotent
  and last-write-wins. Flushing is driven on the `online` event, on app start, and by the service
  worker's Background Sync (`sync` tag `hf-outbox` → `postMessage` to clients) where supported. The
  server's read/star endpoints are already idempotent explicit-value upserts, so no server change
  was needed. The pure queue engine is unit-tested with Vitest (`outbox.test.ts`); server
  convergence is covered by a Rust isolation test.

## Key decisions

- **SQLite, not Postgres/Redis.** A single small binary with one file to back up and low idle
  RAM is exactly right for a home server / Raspberry Pi. WAL + `busy_timeout` + a single-writer
  pool + FTS5 cover the workload without an external database.
- **Per-channel RSS over the YouTube OAuth Subscriptions API for polling.** RSS needs no auth
  and has no quota, which matters for a long-running poller. (A one-time OAuth import helper is
  a possible future add-on, not required to use YouTube feeds.)
- **Reddit metrics via the JSON endpoint, RSS as fallback.** `score`/`comments`/`upvote_ratio`
  aren't in `.rss`, so Digestly uses `.../top.json`; when that's blocked it degrades to `.rss`
  with NULL metrics and logs the bypass.
- **One React build for PWA (and future Tauri).** The UI is never forked between web and
  mobile; native-only capabilities would live in a thin Tauri shell, not a second codebase.
- **Categories, not folders.** A single mandatory grouping concept doubles as the digest
  bucket, which keeps the model simple and the digest grouping obvious.
- **Shared ingest, not per-user duplication.** Polling a popular feed once - regardless of how
  many users subscribe - saves bandwidth, avoids rate-limit trouble, and keeps a single shared
  summary cache; only _state_ is duplicated per user.
- **Runtime-checked sqlx, not the compile-time macros.** Queries use `sqlx::query`/`query_as`
  (not `query!`/`query_as!`). This is a deliberate build choice: there is no live database at
  build time, so the multi-stage Docker build compiles without a `DATABASE_URL` or a checked
  `sqlx-data.json`.
