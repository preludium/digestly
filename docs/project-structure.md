# Project structure

Where things live, and which directory owns which kind of entity. `backend/` is a Rust (axum +
tokio + sqlx over SQLite) service; `web/` is a React 19 + TypeScript + Vite app built with pnpm
and linted/formatted by Biome (not ESLint/Prettier).

See `CONTEXT.md` for the definitions of [[Feed]], [[Item]], [[Digest]], [[Ingest]], and the other
domain terms that appear throughout these paths.

## backend/

`Cargo.toml` and `Cargo.lock` live in `backend/`. `backend/src/` has both top-level module files
and module directories.

Top-level `src/` files:

```
main.rs           entry point: config, wiring, spawns and aborts the background tasks
config.rs         environment/config loading
db.rs             SQLite pool setup
error.rs          shared error type(s)
events.rs         live SSE event bus
healthcheck.rs
http.rs           router assembly
maintenance.rs    retention purge task
opml.rs
query.rs
seed.rs
seed_demo.rs
settings.rs
isolation_tests.rs
```

Module directories under `backend/src/`:

```
routes/     HTTP handlers, one module per resource: admin, ai, auth, categories, digest,
            events, feeds, items, me, notifications, oauth, opml, passkeys, settings (+ mod.rs)
ingest/     feed fetching, parsing, scheduling - scheduler.rs is the ingest background task
ai/         provider-agnostic summarization + transcripts - transcript_worker.rs is a task
digest/     digest building + cron (mod.rs + cron.rs)
auth/       passwords, passkeys/WebAuthn
oauth/      third-party account import
notify/     push notifications (ntfy)
```

Other backend directories:

- `backend/migrations/` - SQLite schema, embedded at compile time via `sqlx::migrate!`.
- `backend/tests/fixtures/` - sample feeds used by tests (`sample_atom.xml`, `sample_rss.xml`,
  `sample_jsonfeed.json`).

## web/

```
src/                      App.tsx, main.tsx, index.css, vite-env.d.ts at top level
src/components/ui/        vendored shadcn primitives - do not edit; excluded from Biome (biome.json)
src/components/common/    shared primitives used across features (SettingsTile, EmptyState,
                           ErrorBanner, Pagination, PageHeadings, TabShell, ConfirmDialog, ...)
src/components/settings/  settings-tab feature components
src/components/feeds/     feed feature components
src/components/items/     item feature components (ItemCard, ItemGrid, ItemPreview, ...)
src/components/health/    feed-health feature components (one component per file)
src/components/           a few app-level shells live directly here: AppShell, AppBanners,
                           ErrorBoundary, Onboarding, PasskeyManager
src/routes/               page components, one per route (Feed, Settings, Health, ...).
                           Route-scoped helpers may be colocated here (e.g. manage.helpers.ts +
                           its .test.ts); cross-route pure utilities go in src/lib/ instead.
src/hooks/                one hook per API/domain (useFeeds, useItems, useDigest, useAuth, ...)
src/lib/                  pure utilities, tests colocated (api.ts, format.ts, topicColor.ts + .test.ts)
src/stores/               zustand stores (ingest.ts, ui.ts)
e2e/                      Playwright specs (auth, feeds, items, digest, settings, ...;
                           support/, screenshots/)
```

There is no `scripts/` directory - do not add one or reference one.

## Placement rule

**A new file goes in the directory whose description above already covers it.** If none does,
that is a signal to discuss the right home - not a licence to invent a new directory.

See `docs/background-tasks.md` for the four long-lived background tasks and where they are
wired up.
