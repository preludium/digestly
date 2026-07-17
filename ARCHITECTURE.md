# Architecture

Digestly is one Rust binary that serves an embedded React SPA, backed by a single SQLite file.
This document maps the end-to-end data flow, the data model, and the key design decisions so an
engineer can navigate the code.

## Tech stack

| Layer       | Choices                                                                          |
| ----------- | -------------------------------------------------------------------------------- |
| Backend     | Rust, `axum` (HTTP), `tokio` (async), `sqlx` over SQLite (WAL + FTS5)            |
| Parsing     | `feed-rs` (RSS 2.0 / RSS 1.0-RDF / Atom / JSON Feed), `ammonia` (HTML sanitize)  |
| HTTP client | `reqwest` (rustls, gzip/brotli, cookies off)                                     |
| Crypto      | `argon2` (passwords), ChaCha20-Poly1305 (secrets at rest, key from `SECRET_KEY`) |
| Frontend    | React 19 + TypeScript + Vite, TanStack Query, Zustand, Tailwind, React Router    |
| Packaging   | Multi-stage Docker image (node build - rust build - `debian-slim` runtime)       |

## Not yet built

These are designed for but not implemented in the current build:

- **Tauri v2 Android app.** The same React build is designed to power a native Android wrapper,
  but no Android target is built yet. See `docs/plans/s2-android.md` and
  `docs/plans/capacitor-android.md` for the implementation plans.
- **Admin aggregate delivery** (email/webhook/file). Per-user ntfy is the only delivery path
  today.

## Single-service model

There is exactly one deployable service on one port with one database file:

- The Rust binary (`digestly`) serves the built SPA via `tower-http::ServeDir` with an SPA
  fallback: a real static file is served when it exists, otherwise `index.html` is returned
  with `200` so React Router deep links survive refresh/back. Unknown `/api/*` paths return a
  `404` JSON, never the SPA shell. See `backend/src/http.rs` (`spa_fallback`).
- `/api/*` is matched first (nested router); everything else falls through to the SPA.
- Storage is a single SQLite file at `${DATA_DIR}/digestly.db` in WAL mode with FTS5.
- The Docker image is a three-stage build (node build - rust build - `debian-slim` runtime)
  and runs on ARM64 and x86-64 (see `docs/adr/0003-single-multi-arch-image-via-buildx.md`).

`backend/src/main.rs` wires everything together at boot: load and validate config, create
`DATA_DIR`, connect and migrate the DB, bootstrap the built-in admin, derive keys from
`SECRET_KEY`, spawn four background tasks (ingest scheduler, transcript worker, digest scheduler,
maintenance), and serve with graceful shutdown on SIGTERM. All four tasks are aborted in the same
shutdown block. See `docs/background-tasks.md` for the spawn/abort sites and per-task mechanics.

For the full directory inventory, see `docs/project-structure.md`.

## Ingestion flow

Ingestion is global and shared - feeds are polled once for all users. A tokio background
scheduler (`backend/src/ingest/scheduler.rs`) runs the loop:

1. **Select due feeds.** `next_fetch_at <= now`, not `disabled`, and having at least one active
   subscription (orphan feeds are skipped). A claim lease and concurrency cap prevent
   re-selection and overload; per-host politeness limits concurrent requests to one in-flight per
   hostname with a minimum delay between them.
2. **Conditional GET.** Sends stored `ETag` / `If-Modified-Since`; on `304` it updates
   `last_fetch_at` and reschedules. Accepts gzip/brotli encoding, sends a real `User-Agent`, and
   applies a per-request timeout.
3. **Parse - sanitize - dedup - store.** `feed-rs` parses RSS/Atom/JSON uniformly; HTML is
   sanitized with `ammonia` (scripts, handlers, `javascript:` URLs stripped) before storing;
   relative URLs are rewritten to absolute; a lead image and reading time are extracted; new
   items are inserted in a transaction (FTS kept in sync by triggers).
4. **Success/failure bookkeeping.** On success: reset `failure_count`, clear `last_error`, set
   the next fetch time. On failure: increment `failure_count`, store `last_error`, and reschedule
   with exponential backoff + jitter (capped ~6h). After repeated failures (or a terminal status
   like `401/403/410`) the feed is disabled and surfaced in `/health`.

Per-feed isolation: each feed fetch is an independent `Result`, logged with `tracing::warn` on
failure - one bad feed never crashes the loop.

**Source specifics** (`backend/src/ingest/`):

- **Reddit:** the `.rss` feed lacks `score`/`comments`, so Digestly prefers the authenticated
  Reddit API when an instance Reddit account is connected, falls back to the public JSON endpoint
  (`.../top.json`), and finally to `.rss` with NULL metrics when JSON is blocked. The bypass is
  always logged. `min_score` is applied at query time since items are shared. See
  `docs/adr/0007-reddit-json-api-with-rss-fallback.md`.
- **YouTube:** polled via per-channel RSS (no OAuth, no quota); handles/`@handle`/channel URLs
  are resolved to a `channel_id`. See `docs/adr/0006-youtube-rss-not-oauth-subscriptions-api.md`.
  Transcripts are fetched by the transcript worker shortly after ingest (see below);
  `transcript_status` becomes `fetched` or `unavailable`.

## Shared-ingest / per-user-state model

The schema splits cleanly into **global** (shared, no `user_id`) and **per-user** tables:

| Global (shared)                                                                 | Per-user (`user_id`, cascade-deleted)                                                     |
| ------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `feeds`, `items`, `items_fts`, `item_summaries`, `ai_providers`, `app_settings` | `categories`, `subscriptions`, `item_states`, `settings`, `user_notifications`, `digests` |

A "feed" in the UI is a `subscriptions` row (a user's link to a global `feeds` catalog entry
with their category and settings). The fetched content (feeds + items) is shared; all state
(read/star, categories, min-score, notifications, digests) is per-user. See
`docs/adr/0010-shared-ingest-per-user-state.md`.

**Scoping is enforced server-side.** Every non-admin query derives `user_id` from the session
(the `CurrentUser` extractor) - it is never a client-supplied parameter. No endpoint reads or
mutates another user's rows, except the admin user-management endpoints (which manage accounts
only, never feed contents). `AdminUser` rejects non-admins with `403` at the server.

### Schema (`backend/migrations/0001_init.sql`)

**Accounts & auth (global):**

- `users(id, username UNIQUE, password_hash, role, disabled, created_at, last_login_at)` -
  `role IN {admin, user}`.
- `passkeys(...)` - WebAuthn credentials (one per authenticator; `public_key` holds the
  serialized `webauthn-rs` credential, `sign_count` mirrors the counter for the clone check).
- `sessions(id, user_id, created_at, expires_at)` - revocable server-side sessions.

**Global content:**

- `feeds(...)` - canonical catalog deduped by normalized `feed_url`; `kind IN {rss, atom,
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
  concept; `Other` is the non-deletable catch-all (see `docs/adr/0009-categories-not-folders.md`).
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
  is derived from `SECRET_KEY` (SHA-512 to 64-byte `cookie::Key`, `SignedCookieJar`), so
  sessions survive restarts but revoke on logout / logout-all / user-delete.
- **Roles** are `admin | user`; extractors gate access (`CurrentUser`, `AdminUser`).
- **Admin bootstrap** (`backend/src/auth/bootstrap.rs`): on every boot, ensure the `admin` user exists
  with the `ADMIN_PASSWORD` hash (re-synced if the env value changed). The built-in admin
  cannot be deleted or demoted, and the instance always keeps at least one admin.
- Login errors are generic (no username enumeration).
- **OAuth import (`backend/src/oauth/`).** Optional per-user linking of YouTube/Reddit to import
  followed channels/subreddits as RSS feeds. Client credentials are instance-level env
  (`GOOGLE_OAUTH_*` / `REDDIT_OAUTH_*`); a provider's feature is hidden unless both are set. The
  authorization-code flow stores only an encrypted refresh token per user (`user_oauth` table,
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
  are enforced explicitly (and mirrored by the library): sign-count regression rejects a
  credential whose signature counter stalls or goes backwards (cloned authenticator), and the
  last-sign-in-method guard refuses to delete a user's only credential when they have no
  password. Because passkeys bind to `RP_ID`, changing the hostname invalidates them.

## Pluggable AI

AI is provider-agnostic and admin-global (`backend/src/ai/`):

- **`LlmClient` trait, two implementations only** (`backend/src/ai/client.rs`): `OpenAICompatibleClient`
  (`POST {base_url}/chat/completions`, covers Groq/OpenAI/Gemini/Mistral/Ollama/most custom)
  and `AnthropicClient` (`POST {base_url}/messages`), selected by `api_style`. There is no
  provider-specific code beyond these two clients.
- **Presets** (Groq / OpenAI / Anthropic / Gemini / Mistral / Ollama) bake in base URL +
  API style; the admin supplies only a key and model. Exposed via `GET /api/ai/presets`.
- **Write-only keys.** Keys are encrypted at rest with a `SECRET_KEY`-derived ChaCha20-Poly1305
  key (`backend/src/ai/crypto.rs`; key = SHA-256 of `SECRET_KEY`, blob = `nonce(12) | ciphertext+tag`).
  Keys are never returned by any endpoint or logged; rotation is delete + recreate.
- **Shared summary cache.** Summaries are written to `item_summaries` keyed by `(item, model)`
  and reused across users, so the same item is never re-summarized (unless the model differs or
  the user forces a refresh).
- **SSRF guard** on custom base URLs rejects private/loopback ranges unless allow-private is
  enabled - but intentionally allows localhost for Ollama (`provider_type == ollama`).
- **Token budget guard** (`backend/src/ai/budget.rs`): daily/monthly token budgets checked before
  a call and recorded after; large source lists are truncated.

## Video - readable

Video items are rendered as text, not a player (`backend/src/ai/transcript.rs`, `summarize.rs`).
The transcript is fetched by the background transcript worker shortly after ingest (woken by the
ingest scheduler's `new_video_trigger`); the active provider produces a structured readable
summary (intro + key-point bullets + takeaways), cached in `item_summaries`. When
`transcript_status = unavailable`, the description is summarized and labelled as such, and the
watch link is surfaced more prominently. The reader renders: AI summary (primary) - collapsible
full transcript - de-emphasized "Watch on YouTube".

**Video-URL path (optional).** An admin can point `ai.video_provider_id` at a Gemini provider
(Settings - AI - YouTube video summaries). Gemini is then sent the video URL itself, so videos
with no captions at all still get a real summary; any failure falls back to the transcript flow.
Gemini-only - it's the only supported API that accepts a video URL as model input - and it bills
video at roughly 100 tokens per second of runtime, so the token budgets drain much faster.

**Just regular videos.** Shorts are filtered at ingest (the channel feed marks them with a
`/shorts/` link). Live recordings can only be identified from YouTube's player data, so they are
ingested normally and then deleted by the transcript worker once it confirms `isLiveContent`.

## Digest engine

`backend/src/digest/mod.rs`: a global/admin cron config (in `app_settings`) drives per-user
digests. The scheduler ticks every 45 s (`SCHED_TICK_SECS`) and fires at most once per matching
minute (a `digest.last_run` stamp guard).

- `DigestConfig` holds `enabled`, `cron`, `lookback_days`, `timezone`, `categories`,
  `ai_enabled`. The cron is a restricted 5-field parser (`backend/src/digest/cron.rs`) matched
  against wall-clock time in the configured timezone (DST-correct), with a `describe()` human
  preview.
- `run_all` resolves the active provider + params once, then iterates users. For each user it
  gathers in-window items grouped by their categories (respecting `min_score`), and produces
  one AI prompt per non-empty category.
- **Raw-titles fallback:** if there is no active provider, or a provider call fails, or the
  budget is exceeded, that section falls back to raw grouped titles + links with a
  `fallback_note`. The run never fails.
- Each digest is archived to the user's `digests` row as a structured `payload_json`
  (categories, sources, `ai_used`, `fallback_note`, `failure_warning`) and, if the user enabled
  digest pushes and has a channel, pushed to their ntfy.
- If more than two of a user's sources failed to fetch in the window, a `failure_warning` is
  included in both the digest and the push.

The per-user window is computed incrementally per run - see
`docs/adr/0004-per-user-incremental-digest-window.md`.

## ntfy notifications

Per-user (`backend/src/notify`): config lives in `user_notifications` (server URL, topic,
encrypted write-only token, priority, per-event toggles). Sending is a plain HTTP
`POST {server}/{topic}` with Title/Priority/Tags (+ auth) headers, a 10 s timeout and one retry;
failures are logged and surfaced, never fatal. The SSRF guard allows the user-configured ntfy
host (often localhost/LAN) while validating the URL. Feed-health pushes are throttled to one per
feed per healthy-to-failing transition and de-duped per subscriber, so a feed shared by many
users notifies each at most once per transition.

## Retention / maintenance

`backend/src/maintenance.rs` runs a periodic purge every 6 h driven by `retention.max_age_days`
and `retention.max_per_feed` in `app_settings` (both `0` = keep forever). Starred items are never
purged - an item starred by any user survives. Deletes cascade to `item_states`/`item_summaries`
and keep FTS in sync via the delete trigger. See `docs/background-tasks.md` for the task's boot
behavior and schedule.

## Frontend

One React app (`web/`) powers the browser, the installed PWA, and future native wrappers
(see `docs/adr/0008-one-react-build-for-web-and-native.md`):

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
- **Offline write-sync:** read/star mutations made offline are applied optimistically to the query
  cache and appended to a persistent outbox (`web/src/lib/outbox.ts`, localStorage-backed; wired
  to the network + React in `web/src/lib/sync.ts`). Each entry carries an explicit value; before
  replay the outbox coalesces per `(kind, item)` to the latest intent, so replay is idempotent
  and last-write-wins. Flushing is driven on the `online` event, on app start, and by the service
  worker's Background Sync (`sync` tag `hf-outbox` - `postMessage` to clients) where supported.
  The server's read/star endpoints are already idempotent explicit-value upserts, so no server
  change was needed. The pure queue engine is unit-tested with Vitest (`outbox.test.ts`); server
  convergence is covered by a Rust isolation test.

## Key decisions index

The design decisions below are each recorded in full as an ADR in `docs/adr/`. This index lists
them with a one-line summary; follow the link for context, rejected alternatives, and consequences.

| ADR | Decision |
| --- | -------- |
| [0001](docs/adr/0001-ci-is-the-only-merge-gate.md) | CI (all three jobs) is the only required merge gate; no exemptions for pre-existing failures |
| [0002](docs/adr/0002-verify-facts-about-the-outside-world.md) | External behavior claims must be verified before becoming premises; unverifiable ones get `ASSUMPTION:` |
| [0003](docs/adr/0003-single-multi-arch-image-via-buildx.md) | A single multi-arch image covers ARM64 + x86-64 via `docker buildx` |
| [0004](docs/adr/0004-per-user-incremental-digest-window.md) | Digest window is per-user and incremental; `lookback_days` is a cap, not a per-run literal |
| [0005](docs/adr/0005-sqlite-not-postgres-redis.md) | SQLite (WAL + FTS5) over Postgres or Redis; single file, no external DB |
| [0006](docs/adr/0006-youtube-rss-not-oauth-subscriptions-api.md) | YouTube feeds polled via per-channel RSS; no OAuth, no API quota |
| [0007](docs/adr/0007-reddit-json-api-with-rss-fallback.md) | Reddit metrics from JSON/OAuth API; RSS is the fallback with NULL metrics |
| [0008](docs/adr/0008-one-react-build-for-web-and-native.md) | One React build for browser, PWA, and future native wrappers; no UI fork |
| [0009](docs/adr/0009-categories-not-folders.md) | Categories are the single grouping concept; no folder hierarchy or multi-label tags |
| [0010](docs/adr/0010-shared-ingest-per-user-state.md) | Feeds fetched once for the instance; only state (read, star, subscriptions, digests) is per-user |
| [0011](docs/adr/0011-runtime-sqlx-not-compile-time-macros.md) | `sqlx::query` (runtime) not `query!` macros; no live DB needed at build time |
