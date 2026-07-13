# Architecture overview

Digestly is a single Rust binary that serves an embedded React SPA, backed by a single SQLite file. This page covers the module layout, the shared-ingest/per-user-state data model, the boot sequence, and the HTTP routing structure.

See the repository's `ARCHITECTURE.md` for an expanded version of this reference.

## Single-service model

One deployable service on one port with one database file:

- The Rust binary (`digestly`) serves the built SPA via `tower-http::ServeDir` with an SPA fallback: a real static file is served when it exists, otherwise `index.html` returns 200 so React Router deep links survive refresh/back. Unknown `/api/*` paths return 404 JSON.
- `/api/*` is matched first (nested router); everything else falls through to the SPA.
- Storage is a single SQLite file at `${DATA_DIR}/digestly.db` in WAL mode with FTS5.
- The Docker image is a three-stage build (node build → rust build → `debian-slim` runtime) and runs on ARM64 and x86-64.

**Source:** `src/http.rs` (router, `spa_fallback`, CORS), `Dockerfile`, `docker-compose.yml`

## Boot sequence (`src/main.rs`)

1. **Subcommand dispatch:** `--healthcheck` runs a TCP probe and exits; `--seed` ingests test fixtures and prints a sample digest.
2. **Tracing init:** `RUST_LOG` env-based filter (default `info,digestly=debug`).
3. **Load `.env`** (no-op in Docker).
4. **`Config::from_env()`** — fails fast if `SECRET_KEY` or `ADMIN_PASSWORD` is missing/invalid.
5. **`db::connect` + `db::migrate`** — opens SQLite, runs migrations (`migrations/` directory).
6. **`auth::bootstrap::run`** — ensures the built-in `admin` user exists, re-syncs password hash if env changed, seeds default categories and `app_settings` defaults.
7. **Derive keys** from `SECRET_KEY`: SHA-512 → 64-byte cookie-signing key; SHA-256 → 32-byte AEAD key for encrypting secrets at rest.
8. **Spawn background tasks:**
   - Ingestion scheduler (`src/ingest/scheduler.rs`) — polls feeds
   - Digest scheduler (`src/digest/mod.rs`) — generates per-user digests on the admin-configured cron
   - Transcript worker (`src/ai/transcript_worker.rs`) — fetches YouTube captions lazily
   - Maintenance task (`src/maintenance.rs`) — periodic retention purge
9. **Bind and serve** with graceful shutdown on SIGTERM.

## Module layout

| Module | Source | Responsibility |
|--------|--------|---------------|
| `config` | `src/config.rs` | Bootstrap env parsing + fail-fast validation |
| `db` | `src/db.rs` | SQLite pool (WAL, `busy_timeout`, `foreign_keys`), migrations, health ping |
| `http` | `src/http.rs` | Router, `AppState`, CORS (Tauri origins), compression, static SPA serving, `/api/health` |
| `error` | `src/error.rs` | `AppError` / `ApiResult` — JSON error responses; secrets never leaked |
| `healthcheck` | `src/healthcheck.rs` | `--healthcheck` subcommand (TCP probe for Docker HEALTHCHECK) |
| `auth` | `src/auth/` | argon2 passwords (`password.rs`), passkeys/WebAuthn (`passkey.rs`), admin bootstrap (`bootstrap.rs`), signed-cookie sessions (`session.rs`), `CurrentUser`/`AdminUser` extractors (`extract.rs`) |
| `oauth` | `src/oauth/` | OAuth import helpers (YouTube/Reddit): authorize/token/refresh, subscription listing, idempotent `reconcile` |
| `ingest` | `src/ingest/` | Discovery, scheduler, fetch, parse, store, content handling, Reddit/YouTube specifics |
| `ai` | `src/ai/` | `LlmClient` trait + two clients (`client.rs`), provider management (`provider.rs`), key crypto (`crypto.rs`), summarize (`summarize.rs`), transcript (`transcript.rs`), budget (`budget.rs`), transcript worker (`transcript_worker.rs`) |
| `digest` | `src/digest/` | Digest engine (`mod.rs`) + restricted cron parser (`cron.rs`) |
| `notify` | `src/notify/` | Per-user ntfy config + sending (digest push, throttled feed-health push) |
| `maintenance` | `src/maintenance.rs` | Periodic retention purge (starred kept forever) |
| `opml` | `src/opml.rs` | OPML import/export round-trip (`roxmltree` for parsing) |
| `routes` | `src/routes/` | REST handlers: admin, ai, auth, categories, digest, feeds, items, me, notifications, oauth, opml, passkeys, settings |
| `query` | `src/query.rs` | Timezone-aware `when` ranges (DST-correct) and sort/filter SQL used by the items API |
| `seed` | `src/seed.rs` | Seeds the six default categories per account |
| `seed_demo` | `src/seed_demo.rs` | `--seed` test-mode command |

## Shared-ingest / per-user-state model

The schema splits cleanly into **global** (shared, no `user_id`) and **per-user** tables:

| Global (shared) | Per-user (`user_id`, cascade-deleted) |
|---|---|
| `feeds`, `items`, `items_fts`, `item_summaries`, `ai_providers`, `app_settings` | `categories`, `subscriptions`, `item_states`, `settings`, `user_notifications`, `digests` |

A "feed" in the UI is a `subscriptions` row (a user's link to a global `feeds` catalog entry with _their_ category and settings). The fetched **content** (feeds + items) is shared; all **state** (read/star, categories, min-score, notifications, digests) is per-user.

**Scoping is enforced server-side.** Every non-admin query derives `user_id` from the session (the `CurrentUser` extractor) — it is **never** a client-supplied parameter. `AdminUser` rejects non-admins with 403.

Accounts are global: `users`, `passkeys`, and `sessions` tables have no `user_id` foreign-key cascading (they are the root of the ownership chain).

## Data model highlights

Full schema in `migrations/0001_init.sql` (core) and `migrations/0002_oauth.sql` (OAuth tokens).

### Global tables

- `feeds` — canonical catalog deduped by normalized `feed_url`; `kind ∈ {rss, atom, jsonfeed, youtube, reddit}`; carries `etag`, `last_modified`, `next_fetch_at`, `failure_count`, `last_error`, `disabled`
- `items` — per feed; `content_html` (ammonia-sanitized) + `content_text`, transcript + status, image/duration/reading-time, Reddit `score`/`comments_count`/`upvote_ratio` (nullable), `dedup_hash`
- `items_fts` — FTS5 over `title, content_text, author`, synced by insert/update/delete triggers
- `item_summaries` — shared AI summary cache, `UNIQUE(item_id, model)`; contains no user-identifying data
- `ai_providers` — admin-managed LLM endpoints; `api_key_enc` encrypted at rest, never returned; exactly one `is_active`
- `app_settings(key, value)` — global admin config (registration toggle, ingestion tunables, digest engine config, AI params, retention)

### Per-user tables

- `categories(id, user_id, name, position)` — `UNIQUE(user_id, name)`; `Other` is the non-deletable catch-all; six seeded on account creation
- `subscriptions(id, user_id, feed_id, category_id NOT NULL, content_type, min_score, full_text_extract, disabled, title_override)` — `UNIQUE(user_id, feed_id)`
- `item_states(user_id, item_id, is_read, is_starred, read_at)` — absence = unread & unstarred; upserted on first interaction
- `settings(user_id, key, value)` — per-user preferences (sort, page size, timezone, theme, density, auto-mark-read)
- `user_notifications(user_id PK, ntfy_server_url, ntfy_topic, ntfy_auth_token_enc, ntfy_priority, notify_on_digest, notify_on_feed_health)`
- `digests(id, user_id, created_at, period_start, period_end, item_count, payload_json, notified, error)`

### Key indexes

`items(feed_id, published_at)`, `items(dedup_hash)`, `items(published_at)`, `feeds(next_fetch_at)`, `subscriptions(user_id)`, `subscriptions(feed_id)`, `subscriptions(user_id, category_id)`, `item_states(user_id, item_id)`, `item_summaries(item_id, model)`, `passkeys(user_id)`

## HTTP routing

The axum router (`src/http.rs`) nests:
- `/api/health` → health check
- `/api/*` → REST resource routers (see `src/routes/mod.rs` for the full list)
- Everything else → `spa_fallback` (static file or `index.html`)

CORS is configured for Tauri origins only (`tauri://localhost`, `http://tauri.localhost`, `https://tauri.localhost`). The web build is same-origin.

Middleware stack: `TraceLayer` → `CompressionLayer` → CORS.

## `AppState`

The shared application state held by every handler (`src/http.rs`):

```rust
pub struct AppState {
    pub pool: SqlitePool,              // SQLite connection pool
    pub static_dir: PathBuf,           // Built frontend assets
    pub index_html: Arc<str>,          // index.html cached at boot
    pub key: Key,                      // Cookie signing key
    pub enc_key: [u8; 32],            // AEAD key for encrypting secrets
    pub http_client: reqwest::Client,  // Shared HTTP client for ingestion
    pub ingest_trigger: IngestTrigger, // Wakes ingestion scheduler
    pub webauthn: Option<Arc<Webauthn>>, // WebAuthn RP (None = disabled)
    pub passkey_ceremonies: CeremonyStore, // In-process ceremony state
    pub oauth: Arc<OAuthSettings>,     // OAuth client credentials
    pub oauth_states: OAuthStates,     // In-process OAuth CSRF state
}
```

## Key decisions

- **SQLite over Postgres/Redis.** Single file, low idle RAM, trivial backup. WAL + `busy_timeout` + single-writer pool suffices for this workload.
- **Per-channel RSS over YouTube OAuth API for polling.** No auth, no quota. One-time OAuth import helper for subscription discovery.
- **Reddit metrics via JSON endpoint, RSS as fallback.** `score`/`comments`/`upvote_ratio` aren't in `.rss`.
- **One React build for PWA (and future Tauri).** UI never forked; native-only capabilities in a thin shell.
- **Categories, not folders.** Single mandatory grouping = digest bucket.
- **Runtime-checked sqlx.** No compile-time query macros; no live DB at build time for Docker multi-stage.
