# Digestly

A self-hosted, multi-user feed reader with an AI digest, packaged as a single Docker container. Digestly polls your RSS/Atom/JSON feeds, YouTube channels, and subreddits once, presents everything as a mobile-first card grid, turns videos into readable text, and pushes you a scheduled, per-user, LLM-summarized digest over your own ntfy channel.

## What it is

- **Multi-user, shared ingest / per-user state.** Feeds are polled _once_ for the whole instance no matter how many people subscribe. All reader state (subscriptions, categories, read/star, notifications, preferences, digest history) is private and scoped to your account. No social features, no cross-user visibility.
- **Card-grid reader.** Responsive grid (1–4 columns), unified filter bar (type/status/when/sort/category chips), full-text search, reading preview. URL-encoded filters survive refresh/back.
- **AI summaries + video-as-text.** Summarize any article on demand; YouTube videos are rendered as an AI summary of the transcript with a collapsible full transcript and de-emphasized "Watch on YouTube" link. Summaries are cached and shared across users, keyed by `(item, model)`.
- **Per-user ntfy notifications.** Each user configures their own ntfy server/topic for digest pushes and feed-health alerts.
- **Scheduled AI digests.** Admin-owned cron drives per-user, category-grouped digests summarized by the active AI provider and archived to each user's history.
- **OPML import/export** and an installable **PWA** with offline reading and queued read/star sync.

## Tech stack

| Layer     | Choices                                                                     |
| --------- | --------------------------------------------------------------------------- |
| Backend   | Rust, `axum` (HTTP), `tokio` (async), `sqlx` over SQLite (WAL + FTS5)      |
| Frontend  | React 19 + TypeScript + Vite, TanStack Query, Zustand, Tailwind 4, shadcn   |
| Packaging | One multi-stage Docker image (ARM64 + x86-64)                               |
| Storage   | Single SQLite file at `${DATA_DIR}/digestly.db`                             |

The Rust binary serves the built React SPA itself (`tower-http::ServeDir` + SPA fallback), so there is exactly one deployable service on one port.

## Quick start

```bash
cp .env.example .env
# Set SECRET_KEY and ADMIN_PASSWORD in .env
docker compose up
```

Open `http://localhost:8080`, log in as `admin` with the password you set.

## Documentation index

- [Architecture overview](architecture/overview.md) — single-service model, module layout, shared-ingest/per-user-state data model, boot sequence
- [Ingestion engine](backend/ingestion.md) — feed polling, parsing pipeline, Reddit/YouTube specifics, failure handling
- [AI & digest engine](backend/ai-and-digest.md) — pluggable AI providers, summarization, transcripts, digest generation
- [Frontend](frontend/overview.md) — React SPA, routes, components, PWA, offline write-sync
- [Auth & operations](auth-and-operations.md) — authentication (passwords, passkeys, roles), deployment, maintenance, OAuth imports

## Key design decisions

- **SQLite, not Postgres/Redis.** Single small binary with one file to back up, low idle RAM. WAL + `busy_timeout` + single-writer pool + FTS5.
- **Per-channel RSS over YouTube OAuth API for polling.** No auth, no quota. One-time OAuth import helper for subscription discovery.
- **Reddit metrics via JSON endpoint, RSS as fallback.** `score`/`comments`/`upvote_ratio` aren't in `.rss`.
- **Shared ingest, not per-user duplication.** Feeds polled once; shared summary cache; only _state_ is per-user.
- **Categories, not folders.** A single mandatory grouping concept doubles as digest bucket.
- **Runtime-checked sqlx, not compile-time macros.** No live DB at build time; multi-stage Docker compiles without `DATABASE_URL`.

## Where key things live

| Area            | Backend source              | Frontend source                   |
| --------------- | --------------------------- | --------------------------------- |
| Entrypoint      | `src/main.rs`               | `web/src/main.tsx` → `App.tsx`    |
| HTTP/routing    | `src/http.rs`, `src/routes/`| `web/src/routes/`                 |
| Ingestion       | `src/ingest/`               | —                                 |
| AI/digest       | `src/ai/`, `src/digest/`    | —                                 |
| Auth            | `src/auth/`                 | `web/src/hooks/useAuth.ts`        |
| Data model      | `migrations/`               | `web/src/lib/types.ts`            |
| PWA/offline     | —                           | `web/public/sw.js`, `web/src/lib/pwa.ts`, `web/src/lib/outbox.ts`, `web/src/lib/sync.ts` |
| Config          | `src/config.rs`             | —                                 |
| Maintenance     | `src/maintenance.rs`        | —                                 |

## Existing documentation

The repository also contains these files worth reading:

- `README.md` — user-facing overview and setup instructions
- `ARCHITECTURE.md` — detailed architecture reference (module layout, ingestion flow, digest engine, frontend, key decisions)
- `TROUBLESHOOTING.md` — common issues and their resolutions
- `TODO.md` — historical task list (mostly completed)
- `prompt.md` — original product specification
- `docs/` — planning docs for Android/Capacitor, example digests, YouTube throttling
