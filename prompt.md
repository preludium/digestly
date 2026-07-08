# Build "Digestly" — a self-hosted RSS/YouTube/Reddit reader + AI digest

Build a complete, production-quality, self-hosted feed aggregator ("Digestly") that runs on a home server as a single Docker container. It is a **multi-user, mobile-first, Feedly-style reader** (browse a card grid, read, star, search, filter/sort) **plus a scheduled AI digest** (LLM-summarized email/webhook/file). Deliver a working, compiling, runnable project — not a sketch.

**Multi-user:** each user has a completely private feed set, categories, read/star state, notification (ntfy) config, preferences, and digests — there are **no interactions between users** (no sharing, no social features). Ingestion, AI, and the digest engine are **shared/admin-managed** (feeds polled once, one AI provider, one schedule); all *reader state* is per-user (see §1a and §2).

This is a **one-shot build**: produce the full repository, every file, wired together and runnable via `docker compose up`. Do not leave TODOs in core paths. Where a real-world edge case exists, handle it (see the Edge Cases section — this is the most important part of the spec).

A companion static UI mockup exists (`mockup.html`) that demonstrates the intended interaction model (card grid, preview sheet, unified filters, provider manager). Treat it as the visual/interaction reference; this document is authoritative for behavior, data, and API.

---

## 1. Tech stack (fixed — do not substitute)

- **Backend:** Rust, `axum` (HTTP), `tokio` (async runtime), `sqlx` (compile-time-checked queries) over **SQLite** (WAL mode), `feed-rs` (RSS 2.0 / RSS 1.0-RDF / Atom / JSON Feed parsing), `reqwest` (HTTP client, gzip/brotli, cookies off), `ammonia` (HTML sanitization), `scraper` + a readability port for full-text extraction, `tracing` (structured logs), `tower-http` (compression, CORS, static file serving), `chrono`/`chrono-tz` (time + timezones), `lettre` (SMTP).
- **Frontend:** React + TypeScript + Vite, built to static assets and **served by the Rust binary** (`tower-http::ServeDir`) so there is exactly one deployable service. Minimal styling (Tailwind or plain CSS). **Mobile-first, responsive PWA** (see §9, §9a) — the same build powers browser, installed PWA, and the Tauri Android app.
- **Mobile:** the same React frontend is (a) an installable **PWA** (manifest + service worker, offline reading, add-to-homescreen) and (b) wrapped in **Tauri v2** to produce a native **Android `.apk`**. Tauri's Rust layer is only a thin native shell (secure token storage, push notifications, biometric unlock) — it is **not** the server; the app is a client talking to the home-server `/api`. Do not duplicate UI code between web and mobile.
- **Storage:** SQLite single file at a configurable path (default `/data/digestly.db`), WAL mode, plus SQLite **FTS5** for full-text search. No external database.
- **Packaging:** One multi-stage `Dockerfile` (build frontend → build Rust → slim runtime image). One `docker-compose.yml` mounting a `./data` volume. Must run on ARM64 (Raspberry Pi) and x86-64. The Tauri Android build is a separate build target/CI job, not part of the server image.

Rationale to honor: single small binary, low idle RAM, one container, one file DB, trivial backup. Do not introduce Postgres, Redis, or a separate frontend server.

**Remote access is out of scope:** the user reaches the server over **Tailscale**. Assume the app is served at a Tailscale hostname (e.g. `https://digestly.<tailnet>.ts.net`). The PWA and Android app require a **secure context (HTTPS)** — document using `tailscale cert` / MagicDNS to obtain a TLS cert so service workers and Android (cleartext-blocked by default) work. Do not build VPN/tunnel/reverse-proxy logic.

---

## 1a. Multi-user, authentication & roles

- **Accounts:** username + password. Passwords hashed with **argon2** (never stored/logged in plaintext).
- **Passkeys (WebAuthn):** users can register one or more **passkeys** (via `webauthn-rs`) and sign in with them passwordless. Password and passkeys are both valid sign-in methods for the same account. The Relying Party ID is the Tailscale hostname (document that passkeys require the stable HTTPS origin).
- **Roles:** `admin` | `user`. **New sign-ups are always `user`.** Roles gate admin-only screens/endpoints.
- **The admin account:** a single built-in user with username **`admin`**, role `admin`, bootstrapped on startup from env **`ADMIN_PASSWORD`** (hashed on first run; if the env value changes, re-sync the hash on boot). The `admin` account cannot be deleted or demoted, and the system must always retain **at least one** admin.
- **Registration:** **open self-signup by default**, controlled by an admin-owned global setting `allow_registration` (default `true`). When off, the register page/endpoint returns a clear "registration disabled" state and only the admin can create accounts.
- **Sessions:** issue a signed token/cookie (signed with `SECRET_KEY`) that works for the PWA (cookie/localStorage) and Tauri Android (Keystore). Every `/api/*` call resolves the current user; **every data query is scoped to that `user_id`** — no endpoint may read or mutate another user's rows (except admin-user-management endpoints, §10).
- **Admin capabilities:** list all users; change a user's role (user↔admin); disable/enable; delete a user (cascades all their per-user data); toggle `allow_registration`. Admins do **not** see other users' feed contents — only account management.
- **Global (admin-only) configuration:** **ingestion**, **AI providers**, and the **digest engine** are instance-wide and editable **only by admins** (one AI key/provider and one fetch cadence serve everyone). Regular users cannot see or change these.
- **Per-user configuration (every user, incl. admins):** their feeds/categories, reading preferences, and **ntfy notifications** (§7a). This is the only settings area a non-admin can edit besides Profile.

---

## 2. Domain model (SQLite schema, via migrations)

Use `sqlx` migrations in `/migrations`. **Global tables** (shared ingestion, no `user_id`) vs **per-user tables** (all state) — the "shared ingest, per-user state" model.

**Accounts & auth**
- `users(id, username UNIQUE, password_hash, role, disabled, created_at, last_login_at)` — `role` ∈ `admin | user`.
- `passkeys(id, user_id, credential_id UNIQUE, public_key, sign_count, name, created_at, last_used_at)` — WebAuthn credentials; a user may have several.
- Sessions: stateless signed token or a `sessions(id, user_id, expires_at)` table — your choice, but revocable on logout / user-delete.

**Global (fetched once, shared by all users — NOT user-scoped)**
- `feeds(id, feed_url UNIQUE, site_url, title, description, icon_url, kind, etag, last_modified, last_fetch_at, next_fetch_at, fetch_interval_secs, failure_count, last_error, disabled, created_at)` — canonical catalog, deduped by **normalized** `feed_url`. `kind` ∈ `rss | atom | jsonfeed | youtube | reddit`. Polled **once** no matter how many users subscribe; a feed with **zero active subscriptions** is not polled (and may be GC'd).
- `items(id, feed_id, guid, url, title, author, content_html, content_text, transcript_text, transcript_status, image_url, duration_secs, reading_time_secs, published_at, fetched_at, score, comments_count, upvote_ratio, dedup_hash)` — global per feed. `transcript_status` ∈ `none | fetched | unavailable`. `score`/`comments_count`/`upvote_ratio` from Reddit JSON (and HN) where available, else NULL. **Read/star is NOT here — it's per-user.**
- `items_fts` — FTS5 over `title, content_text, author`.
- `item_summaries(id, item_id, model, api_style, summary_text, created_at, UNIQUE(item_id, model))` — **shared AI summary cache** so the same item isn't re-summarized; reused for all users.
- `ai_providers(id, name, provider_type, api_style, base_url, model, api_key_enc, is_active, created_at)` — **global, admin-managed** LLM endpoints (§6). `api_key_enc` **encrypted at rest, NEVER returned by the API**. Exactly one active for the whole instance. (Not user-scoped — one AI config serves everyone.)
- `app_settings(key, value)` — **global, admin-only** config: `allow_registration`, ingestion tunables, digest engine config (schedule, look-back, enabled, categories, delivery), AI global params (max_tokens/temperature/timeout/token-budget).

**Per-user (every row has `user_id`, cascade-deleted with the user)**
- `categories(id, user_id, name, position, created_at, UNIQUE(user_id, name))` — **the single grouping concept = topic group = digest bucket.** No folders. Every subscription has exactly one, mandatory category. Seeded per user on account creation: `AI`, `Software Engineering`, `Finance`, `Politics`, `Lifestyle`, `Other`. `Other` is the per-user non-deletable catch-all.
- `subscriptions(id, user_id, feed_id, category_id NOT NULL, content_type, min_score, full_text_extract, disabled, title_override, created_at, UNIQUE(user_id, feed_id))` — a user's link to a **global** feed with **their** category + settings. This is what the UI calls "a feed." `content_type` ∈ `reading | video` (default from feed kind: youtube → video, else reading; overridable per subscription) — drives the Reading/Videos filter. `min_score` — Reddit floor (default 0; seeded Reddit subs at 50), applied at **query time** since items are shared. `full_text_extract` — per-subscription readability toggle. Deleting a category reassigns its subscriptions to `Other` (confirm in UI); `Other` cannot be deleted.
- `item_states(user_id, item_id, is_read, is_starred, read_at, PRIMARY KEY(user_id, item_id))` — per-user read/star. Absence = unread & unstarred; upsert on first interaction (don't pre-create a row per item).
- `settings(user_id, key, value, PRIMARY KEY(user_id, key))` — per-user *preferences only* (sort, content view, page size, timezone, theme, density, auto-mark-read). No engine config here (that's `app_settings`, admin-only).
- `user_notifications(user_id PRIMARY KEY, ntfy_server_url, ntfy_topic, ntfy_auth_token_enc, ntfy_priority, notify_on_digest, notify_on_feed_health, created_at)` — **per-user ntfy config** (§7a). `ntfy_auth_token_enc` encrypted at rest, never returned. Booleans gate which events fire.
- `digests(id, user_id, created_at, period_start, period_end, item_count, payload_json, notified, error)` — per-user digest history (content is per-user; the *schedule/config* is global/admin).

**Indexes:** `items(feed_id, published_at)`, `items(dedup_hash)`, `items(published_at)`, `items(score)`, `items(comments_count)`, `feeds(next_fetch_at)`, `subscriptions(user_id)`, `subscriptions(feed_id)`, `subscriptions(user_id, category_id)`, `item_states(user_id, item_id)`, `item_summaries(item_id, model)`, `categories(user_id)`, `passkeys(user_id)`.

> Terminology: the UI/API word "feed" (a user's subscription) maps to a `subscriptions` row over the global `feeds` catalog. Read/star, categories, min-score, and AI are all per-user; the fetched **content** (feeds + items) is shared.

---

## 3. Data sources & how to fetch them

### RSS / Atom / JSON Feed (generic)
- Parse with `feed-rs` (handles all uniformly). Seed feeds (assign categories on seed):
  - `https://news.ycombinator.com/rss` → *Software Engineering*
  - `https://www.reddit.com/r/programming/.rss` → *Software Engineering*
  - `https://www.reddit.com/r/MachineLearning/.rss` → *AI*
  - `https://www.reddit.com/r/softwareengineering/.rss` → *Software Engineering*

### YouTube — **do NOT use the OAuth Subscriptions API for polling.**
- Poll **per-channel RSS**: `https://www.youtube.com/feeds/videos.xml?channel_id=<ID>`. No auth, no quota; includes title/link/published/thumbnail.
- **One-time OAuth import helper** (optional, behind a flag): call `subscriptions.list` once, resolve each to `channel_id`, create per-channel RSS feeds. After import, polling uses RSS only. If OAuth isn't configured the feature is hidden — the app is fully usable by pasting channel URLs/handles.
- Accept channel input as URL, `@handle`, or `channel_id`; resolve handles by fetching the channel page and extracting the id.
- **Transcript fetch (core feature — read instead of watch):** for each new video, fetch captions (YouTube `timedtext`/caption track; prefer manual, fall back to auto-generated; user language then any). Store in `transcript_text`, set `transcript_status`. Feeds §6a. If no captions → `transcript_status = unavailable`, fall back to description. Timeout + size cap; do it lazily/async so a slow transcript never blocks other feeds.

### Reddit — **score/comments are NOT in the `.rss` feed.**
- Use the JSON endpoint (e.g. `https://www.reddit.com/r/<sub>/top.json?t=week&limit=<n>`) to get **`score`, `num_comments`, `upvote_ratio`**, then apply the per-feed `min_score` filter and store all three.
- Send a descriptive `User-Agent` (Reddit blocks generic/empty agents); honor `429`/`Retry-After` with backoff. Optionally document a free Reddit OAuth "script" app for higher rate limits (same optional pattern as YouTube import).
- If JSON is blocked/unavailable → fall back to `.rss` with `score/comments = NULL` and skip the score filter (log that it was bypassed — no silent behavior).
- Read text posts' `selftext`/`selftext_html` so they're readable in-app without clicking out.

---

## 4. Ingestion engine (the heart of the app)

Ingestion is **global and shared** — feeds are polled once for all users, never per-user. A `tokio` background scheduler that:
1. Selects feeds where `next_fetch_at <= now`, `disabled = 0`, **and having ≥1 active subscription** (skip orphan feeds), respecting a global concurrency cap (e.g. 8) **and** per-host politeness (min delay per hostname, one in-flight request per host).
2. Fetches with **conditional GET**: send stored `ETag`/`If-Modified-Since`; on `304` do nothing but update `last_fetch_at` and reschedule. `Accept-Encoding` gzip/brotli. Real `User-Agent`. Request **timeout** (default 20s).
3. Parses, normalizes, dedups, **sanitizes HTML**, extracts full text where configured, captures metrics (Reddit score/comments), inserts only new items (transaction; update FTS).
4. On success: reset `failure_count`, clear `last_error`, set `next_fetch_at = now + fetch_interval_secs`.
5. On failure: increment `failure_count`, store `last_error`, reschedule with **exponential backoff + jitter** (cap ~6h). After N consecutive failures, mark `disabled` and surface in the UI (never silently drop).

Ingestion must never crash the process: one bad feed is isolated (`Result` per feed, `tracing::warn`).

---

## 5. Full-text & content handling

- Store both `content_html` (sanitized) and `content_text` (stripped, for FTS + AI).
- **Optional full-article extraction:** per-feed toggle — if a feed only gives summaries, fetch the article URL and run readability extraction. Timeout + size cap; fall back to the feed's own content on failure.
- Rewrite relative URLs in content to absolute (base = item link).
- Extract a lead image (`media:content`, `enclosure`, OG `og:image`, YouTube thumbnail, or first `<img>`) for the card grid.
- Compute `reading_time_secs` (~200 wpm) from readable text/summary.

---

## 6. AI summarization — pluggable provider (on-demand + digest)

AI is **provider-agnostic** and **global/admin-managed**. The **admin** configures one or more providers in Settings → AI (admin-only) and picks the **active** one; every user's summaries/digests use that instance provider + key. Generated summaries are written to the **shared `item_summaries` cache keyed by (item, model)** and reused for all users (no duplicate token spend). Regular users never see AI settings or keys.

- **Predefined providers (base URL + API style baked in — user only supplies a key):** `Groq`, `OpenAI`, `Anthropic (Claude)`, `Google Gemini`, `Mistral`, and `Ollama (local)` (no key). Expose these as templates via `GET /api/ai/presets` so the UI never hardcodes them.
- **Custom endpoint:** user provides `name`, `base_url`, `api_style`, key, `model`.
- **API styles:** `openai` (OpenAI-compatible `POST {base_url}/chat/completions` — covers Groq/OpenAI/Gemini-OpenAI/Mistral/Ollama/most custom) and `anthropic` (`POST {base_url}/messages`). Implement an `LlmClient` trait with **two implementations** (`OpenAICompatibleClient`, `AnthropicClient`) selected by `api_style`. No provider-specific code beyond these two clients.
- **Write-only keys (security requirement):** keys are submitted on create, **encrypted at rest** (`api_key_enc`, using a master key from env `SECRET_KEY`), and **never returned by any endpoint or logged**. The UI shows only "key saved · hidden". To change a key, the admin **deletes the provider and creates a new one** (no key edit/read path exists).
- **Admin-only:** all provider management + AI global params live behind admin auth. Non-admins have no AI settings UI or endpoints.
- **Config per provider:** `model`, plus global `max_tokens`, `temperature`, request timeout, and a daily/monthly **token budget guard**.
- **On-demand per article:** endpoint summarizes one item; **cache** in `item_summaries(item_id, model)`. Check the cache first (any user's entry for that model counts); never re-summarize unless the model differs or the user forces refresh.
- **Digest:** group last-N-days items **by category**, one prompt per non-empty category ("Summarize these developments in 3–4 concise bullets, focus on what's NEW and IMPORTANT" + titles/sources), via the active provider.
- **Fallbacks & guards:** provider error / budget exceeded → produce output with **raw grouped titles/links, no AI** (never fail the digest; for on-demand, return a clear error). Retry with backoff on 429/5xx. Count tokens; truncate huge item lists.
- **Test connection:** `POST /api/ai/providers/{id}/test` does a tiny live call and reports ok/error (never echoes the key).

### 6a. Video → readable (the "read, don't watch" feature)
The product goal is to train the user to **read instead of watch**. Video items are presented as **text**, not an embedded player.
- For each video, run the instance's active provider on `transcript_text` to produce a **structured readable summary** (short intro + key-point bullets + takeaways), target ~1–3 min read. Cache in `item_summaries(item_id, model)`; compute `reading_time_secs` on the item.
- Runs **on-demand when opened** (cached), or eagerly for digest items. Never re-run once cached unless model changed or forced.
- `transcript_status = unavailable` → summarize the description, clearly labelled as description-based; only then surface the watch link more prominently.
- Long transcripts: chunk/condense within the token budget; note if truncated.
- The reader UI (§9.1) renders: **AI summary (primary) → collapsible full transcript → secondary "Watch on YouTube" link**. No embedded/autoplay player.

---

## 7. Digest generation & delivery

The digest **engine is global/admin-configured** (schedule cron, look-back window, enabled, categories included, AI on/off) — a single cadence for the instance (default weekly, Monday 09:00; plus manual admin trigger). But **content is per-user**: on each run the scheduler iterates users and builds each one a digest of **their own** subscriptions, grouped **by their categories**, summarized via the instance AI provider, and archived to their `digests` row (`item_count` + payload).

**Delivery per user is via their own ntfy channel (§7a)** — after a user's digest is built, send them a summary notification (article counts). Optional admin-global delivery of an aggregate/admin digest via email/webhook/file may also be provided, but the primary per-user delivery is ntfy.
- If a user has no ntfy configured, their digest is still generated + archived (viewable in the UI), just not pushed.
- **Alert:** if > 2 of a user's sources failed to fetch in the window, include that in their digest + notification.

Formatting (for the in-app digest view): header `Digest — <date>`, one section per non-empty **category** (name → AI summary → source links), footer listing data sources.

### 7a. Notifications (ntfy) — per-user
Every user (admin or not) can configure a personal **ntfy** channel and receive push notifications. The user runs their own ntfy (self-hosted or ntfy.sh) — Digestly only needs the inputs; **do not bundle or assume a server**.
- **Inputs (per user, in Settings → Notifications):** `ntfy_server_url` (e.g. `https://ntfy.example.ts.net` or `http://localhost:80`), `topic` (channel name), optional `auth_token` (bearer/basic — stored encrypted, write-only), `priority`, and per-event toggles.
- **Events:**
  - **After each digest run:** a summary — e.g. title "Digestly digest" body "42 new articles across AI (14), Software Engineering (12), Finance (6)…" with a click action linking to the app.
  - **Feed health:** when one of the **user's** subscribed feeds becomes failing/disabled, notify (throttled — one notification per feed per state change, not every poll).
- **Sending:** ntfy is a simple HTTP `POST {server}/{topic}` with title/priority/tags/click headers (+ auth header if set). Timeout + one retry; failures are logged and surfaced in the UI, never crash the run.
- **Test button:** `POST /api/notifications/test` sends a "Digestly test notification" to the user's configured channel and reports ok/error (never echoes the token).
- **SSRF note:** ntfy is commonly on localhost/LAN — the SSRF guard must **allow the user-configured ntfy host** (like Ollama), while still validating input.

---

## 8. Configurable scope (explicit requirement)

All per-user settings are editable from that user's **Settings UI** (persisted per-user); admin-only settings (e.g. `allow_registration`) live in `app_settings`. Nothing user-tunable lives only in env vars:
**Per-user (any user can edit their own):**
- **Per-feed (their subscription):** `category` (required), `content_type` override, `min_score` (Reddit), `full_text_extract`, `disabled`, title override.
- **Categories:** create / rename / delete (reassign to `Other`) / reorder.
- **Preferences:** page size (default 50), default sort, default content-type view, timezone, theme, list density, auto-mark-read-on-leaving-page.
- **Notifications (ntfy):** server URL, topic, auth token, priority, per-event toggles + test (§7a).

**Admin-only (global, in `app_settings`/`ai_providers`):**
- `allow_registration` toggle; user management (§9.13).
- **Ingestion:** default `fetch_interval`, per-host politeness delay, global concurrency cap, retention (purge older than N days AND/OR keep max M per feed — **starred kept forever**), SSRF allow-private toggle.
- **AI:** manage providers (add predefined/custom, delete, set active, test), model, global max_tokens/temperature/timeout/token-budget.
- **Digest engine:** schedule (cron), look-back window, enabled, categories, optional admin email/webhook/file delivery.

**Env vars (bootstrap only):** `DATA_DIR`, `BIND_ADDR`, `SECRET_KEY` (required — encrypts provider/ntfy secrets & signs sessions/tokens), **`ADMIN_PASSWORD`** (required — bootstraps the built-in `admin`), optional SMTP + `WEBHOOK_URL` + `RECIPIENT_EMAIL` defaults, optional `GOOGLE_OAUTH_*` for the YouTube import helper. **No `GROQ_API_KEY`** — AI is configured by admin in the UI.

---

## 9. Web/mobile UI (React) — mobile-first, Feedly-like

Build **every** screen below. One responsive React app for browser + PWA + Tauri — do not fork the UI. Client-side routing (React Router); non-API routes fall back to `index.html` (SPA). For every screen implement **loading, empty, and error** states — never a blank screen. **Mobile-first**: design for phone width first, scale up with breakpoints.

### 9.0 App shell (chrome around all authed routes)
- **Top bar:** ☰ menu (mobile), logo, search icon/box (`/` focuses), **Add feed** (`＋`), **refresh-all**, unread badge. Compact on mobile.
- **Navigation drawer** (slide-in on mobile behind ☰; **persistent left sidebar at ≥820px**): **All items**, **Manage categories & feeds**, **Digests**, **Feed health** (red dot + count when any feed failing/disabled), **Settings**, **Profile**, and — **only for admins** — **Users**. A small footer shows the signed-in username + a logout action. Filtering/sorting does **not** live here — it's in the feed header.
- **States:** loading skeleton; first-run empty state ("No feeds yet — Add your first feed") with CTA.

### 9.1 Feed — the core screen — `/`
Single layout: a **responsive card grid** (1 col on phone → 2 → 3 → 4 as width grows). **No list view, no infinite scroll.**
- **Cards (tiles):** lead-image thumbnail (video → thumbnail with ▶ + duration badge; article without image → placeholder), title, feed name + relative time, 2-line snippet, and a footer of pills: **content-type + reading-time** (`📖 6 min` / `🎬 2 min read`), **category**, and where present **`▲ score`** and **`💬 comments`**. Read cards are dimmed; unread marked with a dot.
- **Unified filter bar** (sticky), all facets styled consistently, combinable:
  - **Type:** All / 📖 Reading / 🎬 Videos
  - **Status:** All / Unread / ★ Starred
  - **When:** All time / Today / Yesterday / This week / This month (Today & Yesterday = that calendar day; week/month = last 7/30 days, in the user's timezone)
  - **Sort:** Newest · Oldest · **Quickest read** (reading-time ↑) · **Most popular** (score ↓) · **Most discussed** (comments ↓) · Unread first
  - **Category chips** (horizontally scrollable) with **live counts** that reflect the other active facets: `All topics · AI · Software Engineering · Finance · Politics · Lifestyle · Other`
- **Collapsible filters on mobile:** below ~820px the whole facet bank collapses behind a **⚙ Filters** button showing a **count badge** + a one-line **active-filter summary** and a **Clear** action; tap to expand the panel. At ≥820px filters are always inline.
- **Pagination (NOT infinite scroll):** numbered prev/next + "Page X of Y"; page size from settings. Any filter/sort change resets to page 1. Persist page + active filters in the URL query (`?type=&status=&when=&cat=&sort=&page=`) so state survives refresh/back and is shareable.
- **Result line:** "N items · sorted by <sort>".
- **Auto-mark-read:** optional "mark page read on leaving" (toggle).
- **Open a card → Preview (§9.1a).**
- **Keyboard (desktop):** `n/p` change page, `o` open original, `m` toggle read, `s` star, `r` refresh, `/` search.
- **States:** grid skeletons; empty ("Nothing matches these filters 🎉"); error banner on load failure.

### 9.1a Preview (opened from a card)
The reading surface. **Full-screen overlay on mobile; right-hand side sheet on desktop (≥820px).** Bar with back (`←`), star, mark-read.
- **Reading items:** kicker (`📖 Reading · <category>`), title (links to original), byline (feed · time · reading-time · `▲ score` · `💬 comments` when present), actions (Open original, Summarize/Regenerate), then full **sanitized** content.
- **Video items — as text, not a player:** kicker (`🎬 Video · <category> · shown as text`), title, byline (feed · time · reading-time · video duration), then in priority order:
  1. **AI summary** (primary) — intro + key-point bullets + takeaways.
  2. **Collapsible "📄 Show full transcript"** — the raw captions (or a "no captions available" note when `transcript_status = unavailable`, with description-based summary labelled as such).
  3. **Secondary "▶ Watch on YouTube"** — small, dashed, deliberately de-emphasized.

### 9.2 Search — `/search?q=...`
- Reuses the card grid + pagination; header shows query + result count and the same facets (Type/Status/When/Category) + Sort. Debounced input, matched-term highlighting, "clear filters".
- **States:** idle ("type to search"), loading, empty ("No results for '…'").

### 9.3 Add / Subscribe — modal (bottom sheet on mobile)
- Single input: **site URL, feed URL, YouTube channel/@handle, or subreddit** → `POST /api/feeds/discover`.
- Show discovered candidates (title/type/icon) to pick one or more.
- **Category selector is REQUIRED** — dropdown of all categories including **Other** (the lazy catch-all), with inline "create category". Submitting without a category is blocked (field error). Content-type is auto-detected (YouTube → video) but overridable; Reddit shows **min-score**; advanced (collapsed): interval + full-text toggle. Confirm → subscribe.
- **States:** discovering spinner; multiple-found pick list; none-found (error + "enter feed URL directly"); already-subscribed (warn + link); invalid URL; unreachable host; missing-category error.

### 9.4 Feed settings / edit — modal
- Fields: title override, **category** (required), content-type, fetch interval, min-score (Reddit), full-text toggle, enable/disable, feed URL (read-only + "re-discover"). Shows last-fetch time, last error, failure count.
- Actions: save, **refresh now**, **mark all items read**, **unsubscribe/delete** (confirm).

### 9.5 Manage categories & feeds — `/manage`
The structural hub (scales to many feeds).
- **Top toolbar:** `＋ Add feed`, `＋ New category`, `⬆ Import OPML`, `⬇ Export OPML`.
- **Filter controls:** a **"Show category"** dropdown (All / each category) to narrow to one category, and a **feed-name search** box — both needed once a category holds many feeds.
- **One card per category** (respecting the filter): header with name, feed count, and **Rename / Add feed / Delete** (delete reassigns feeds to `Other`; `Other` not deletable). Under it, **feed rows**: source-type icon (📄 RSS · 🎬 YouTube · 👽 Reddit), name, meta line (kind · content-type · interval · unread count · `min ▲` pill for Reddit), and per-feed actions **⟳ refresh · ✎ edit · ↔ move category · 🗑 unsubscribe**.
- Drag feeds between categories (desktop). Every feed always shows its (mandatory) category.

### 9.6 Feed health / diagnostics — `/health`
- Table: feed, status (OK/failing/disabled), last success, failure count, last error, next retry. Filter "problems only". Actions: retry now, re-enable, edit, unsubscribe. Empty state "All feeds healthy". Failing/disabled feeds are surfaced here — never silently dropped.

### 9.7 Settings — `/settings` (tabbed; tabs shown depend on role)

**Everyone (per-user tabs):**
- **General:** default sort, default content-type view, page size, timezone, list density, auto-mark-read-on-leaving-page, theme.
- **Notifications (ntfy):** server URL, topic, optional auth token (password field, write-only), priority, and per-event toggles — **"Notify after digest"** and **"Notify on feed health issues"**. A **Test** button (`POST /api/notifications/test`) sends a test push and shows ok/error. Explanatory copy: "Digestly pushes to your own ntfy server."
- **Import/Export:** OPML import (upload → preview → confirm; each imported feed needs a category, default `Other`), OPML export, DB backup note, optional YouTube OAuth import helper.

**Admin-only tabs (hidden for regular users; endpoints also enforce role):**
- **Ingestion:** global concurrency cap, per-host politeness delay, default fetch interval, retention policy (purge older than N days / keep max M per feed — starred kept forever), SSRF allow-private toggle.
- **AI:** **provider manager** (global) —
  - list of configured providers: name · provider type · API style · model · **🔒 key saved · hidden** (or "no key (local)"), with a **radio to choose the active** provider and a **🗑 Delete**.
  - **＋ Add provider** → modal: pick a **predefined provider** (Groq/OpenAI/Anthropic/Gemini/Mistral/Ollama) → shows baked-in endpoint + API style (read-only) and asks only for **API key** (+ model); or **Custom endpoint** → name + base URL + **API style (OpenAI-compatible / Anthropic)** + secret key + model. Keys are `password` inputs, submitted once, never rendered back.
  - global: max_tokens, temperature, request timeout, token budget. **Test** button per provider.
- **Digest engine:** enable, cron schedule with human preview ("Every Monday 09:00"), look-back window, categories included; optional admin email/webhook/file delivery; **Run digest now** (runs for all users). Uses the global active AI provider; per-user delivery is via each user's ntfy (§7a).

- **Profile/Account** lives on its own page (§9.12). **Each tab:** dirty-state save bar, validation errors, test-connection results, success toasts.

### 9.8 Digests list — `/digests`
- List: period, delivery-status icons (email/webhook/file ✓/✗), item count, error flag. Actions: open detail, **run manual digest** (period picker), delete old. Empty ("No digests yet"), in-progress indicator.

### 9.9 Digest detail — `/digests/:id`
- Header (period, generated-at); per-**category** section (name → AI summary → source links); footer (data sources); **fetch-failure warning** if >2 sources failed; delivery status per channel; **raw-fallback note** if AI was unavailable. Actions: re-deliver (email/webhook), download JSON, copy.

### 9.10 Login — `/login`
- **Username + password** form → submit. Plus a **"Sign in with a passkey"** button (WebAuthn ceremony). Wrong-creds error, optional rate-limit note, redirect to intended page after login.
- Link to **Register** (shown only when `allow_registration` is on). Generic errors (no username enumeration).

### 9.10a Register — `/register`
- Username + password (+ confirm) → creates a **role=`user`** account, seeds their default categories, logs them in, routes to onboarding. Username-taken + weak-password validation.
- If `allow_registration` is off: show "Registration is disabled — ask the admin for an account." Endpoint also returns 403.

### 9.11 First-run onboarding (once per new account)
- Optionally subscribe to a starter set or import OPML, set timezone + optional digest email, optionally add an AI provider, and **add a passkey**. Skippable. (Runs for every newly-created account, not just the first user.)

### 9.12 Profile — `/profile`
- Shows **username** and **role** (read-only). **Change password** (requires current password). **Passkeys:** list registered passkeys (name, created, last-used) with **add new passkey** (WebAuthn registration), rename, and delete. **Logout everywhere** (revoke sessions). Optional **delete my account** (confirm; cascades all the user's data) — disabled for the `admin` account.

### 9.13 Users (admin only) — `/admin/users`
- Guarded: non-admins get 403/redirect. Table of **all users**: username, role, status (enabled/disabled), created, last login, #subscriptions. Actions: **change role** (user↔admin), **disable/enable**, **delete** (confirm; cascades their data). A global **"Allow open registration"** toggle (writes `app_settings.allow_registration`).
- Guardrails: cannot delete or demote the built-in `admin`; cannot remove the **last** admin; admins manage accounts only — **never** see another user's feed contents.

### 9.14 Global states (everywhere)
- **404** page; **React error boundary** (crash → friendly fallback + reload); **offline / API-unreachable** banner; **toast** system for async feedback.

## 9a. PWA + Tauri v2 Android
- **PWA:** `manifest.webmanifest` (name, icons, theme color, `display: standalone`) + **service worker** for app-shell caching (installable, opens offline) and **offline reading** of already-fetched items/content (cache API responses + content; queue read/star mutations and sync when back online). Add-to-homescreen; "update available" prompt on new SW.
- **Tauri v2 Android:** wrap the **same** built React app. Rust shell adds native-only capabilities: **secure token storage** (Android Keystore), **push/local notifications** (new digest / high-priority items), **biometric unlock**. Ships a real `.apk`. API base URL is user-editable in-app on first launch (Tailscale hostname; no localhost on phone).
- **Networking:** all clients hit `/api` over HTTPS at the Tailscale hostname; expired/invalid token → Login; server-unreachable → offline banner + cached content.
- **Deliverable:** PWA works from the browser build with no extra service; Tauri Android is a separate build target with its own README.

---

## 10. API (REST/JSON, served by axum)

Clean REST API the frontend consumes; also usable headless. Include at least:

**Items**
- `GET /api/items` — filters: `type` (all|reading|video), `status` (all|unread|starred), `category` (all|id), `feed` (id), `when` (all|today|yesterday|week|month — server computes ranges in the user's timezone), `q` (search), `sort` (new|old|quick|top|discussed|unread), `page`, `page_size`. Returns `{ items:[…], page, page_size, total_pages, total_count }`. Each item includes: id, feed_id, category, feed_title, kind, content_type, title, url, author, snippet, image_url, published_at, is_read, is_starred, reading_time_secs, duration_secs, score, comments_count, upvote_ratio, transcript_status, has_summary. **Offset/limit pagination, not cursor/infinite.**
- `GET /api/items/{id}` — full item (content_html / resolved summary from `item_summaries` / transcript_text as applicable; `is_read`/`is_starred` reflect the current user's `item_states`).
- `POST /api/items/{id}/read` · `POST /api/items/{id}/star` (toggle/set).
- `POST /api/items/{id}/summarize` (works for reading + video/transcript; `?force=1` to regenerate).
- `GET /api/categories/counts?type=&status=&when=` — per-category counts for the chips (respecting active facets).

**Categories**
- `GET/POST /api/categories`, `PATCH/DELETE /api/categories/{id}` (delete reassigns feeds to `Other`; `Other` protected).

**Feeds**
- `GET/POST/PATCH/DELETE /api/feeds` (POST/PATCH require a valid `category_id`), `POST /api/feeds/discover` (URL→candidates), `POST /api/feeds/{id}/refresh`, `GET /api/feeds/health`.

**AI providers (admin-only)**
- `GET /api/ai/presets` — predefined templates (name, base_url, api_style, default model, needs_key).
- `GET /api/ai/providers` — list **without keys** (id, name, provider_type, api_style, base_url, model, has_key, is_active).
- `POST /api/ai/providers` — create (key in body; stored encrypted, write-only).
- `PATCH /api/ai/providers/{id}` — edit name/model only (**never** key).
- `POST /api/ai/providers/{id}/activate` · `POST /api/ai/providers/{id}/test` · `DELETE /api/ai/providers/{id}`.
- All AI endpoints require `role=admin`.

**Notifications (ntfy, per-user)**
- `GET/PUT /api/notifications` — the current user's ntfy config (returns everything **except** the auth token; PUT accepts a token write-only).
- `POST /api/notifications/test` — send a test push to the user's channel; report ok/error (no token echo).

**Auth & account**
- `POST /api/auth/register` (guarded by `allow_registration`), `POST /api/auth/login` (username+password), `POST /api/auth/logout`.
- Passkeys (WebAuthn): `POST /api/auth/passkey/login/options` + `.../login/verify`; `POST /api/passkeys/register/options` + `.../register/verify` (authed); `GET /api/passkeys`, `PATCH /api/passkeys/{id}` (rename), `DELETE /api/passkeys/{id}`.
- `GET /api/me` (username, role), `PATCH /api/me` (change password — requires current), `DELETE /api/me` (delete own account; blocked for `admin`).

**Admin (role=admin only)**
- `GET /api/admin/users`, `PATCH /api/admin/users/{id}` (role, disabled), `DELETE /api/admin/users/{id}` — with last-admin / built-in-admin guardrails. `GET/PUT /api/admin/settings` (e.g. `allow_registration`).

**Settings / digest / opml / health**
- `GET/PUT /api/settings` (current user's preferences), `POST /api/opml/import`, `GET /api/opml/export`, `GET /api/digest`, `GET /api/digest/{id}` (current user's history), `GET /api/health`.
- `POST /api/digest/run` — **admin-only**; runs the digest for all users. (Users view their own results via `GET /api/digest`.)

**Scoping & security:** every non-admin endpoint operates on the **authenticated user's** rows only (feeds = their subscriptions; items joined with their `item_states` + `min_score`; categories/notifications/settings/digests per-user; AI/ingestion/digest-engine are admin-global). The server derives `user_id` from the session — it is **never** a client-supplied parameter. Sessions/tokens are signed with `SECRET_KEY` and work for the PWA (cookie/localStorage) and Tauri Android (Keystore). Static UI + `/api/*` on one port; CORS only for the Tauri origin. **Provider API keys are never returned or logged.** Admin user-management endpoints manage accounts only and never expose another user's feed data.

---

## 11. Edge cases — RESOLVE THESE, do not skip (most important section)

**Feeds & parsing**
- Formats: RSS 2.0, RSS 1.0/RDF, Atom, JSON Feed via `feed-rs`; degrade gracefully on partially malformed XML.
- Encoding: honor declared/HTTP charset; handle non-UTF-8 without panicking.
- Missing/garbage `published` → fall back to `updated`, then `fetched_at`; never null where ordering needs it.
- No GUIDs → dedup by `url`, then hash of `title+content`. Same GUID + changed content → update, not new.
- HTTP: `301/308` → follow + persist new `feed_url`; `410` → auto-disable w/ reason; `404` → backoff then disable; `429`/`Retry-After` → honor; `401/403` → disable w/ clear error. Cap redirect loops.
- Giant feeds/items: cap body + item content length; paginate inserts.
- Duplicate subscription (same URL, http vs https, trailing slash) → normalize + dedupe at add time.
- Discovery: `<link rel="alternate" type="application/rss+xml|atom+xml|json">`, common paths (`/feed`, `/rss`, `/atom.xml`, `/feed.json`), plus YouTube/Reddit special-casing.

**Categories**
- Every feed must have a category (DB `NOT NULL` + API validation + UI required field). OPML import / discovery default missing categories to `Other`.
- `Other` is non-deletable; deleting any other category reassigns its feeds to `Other`.

**Security**
- **Sanitize all feed HTML with `ammonia`** before storing/serving (XSS vector — strip scripts, handlers, `javascript:` URLs, disallowed iframes). Strict CSP.
- **AI provider keys:** encrypted at rest with `SECRET_KEY`; never returned by API, never logged; changeable only via delete+create.
- SSRF guard on discovery/full-text/webhook/**custom AI base URLs**: reject private/loopback/link-local ranges unless explicitly allowed (setting, default deny) — but **allow-list localhost for Ollama** intentionally when an Ollama provider is configured.
- Never log secrets (provider keys, SMTP creds).

**Multi-user & auth**
- **Strict per-user scoping:** derive `user_id` from the session on every request; never trust a client-supplied user id. No query may return another user's subscriptions, states, categories, providers, or digests. Cover with tests.
- **Admin bootstrap:** on startup ensure the `admin` user exists with the `ADMIN_PASSWORD` hash; if the env value changed, re-hash on boot. Fail fast if `ADMIN_PASSWORD`/`SECRET_KEY` are missing.
- **Last-admin / built-in-admin guard:** cannot delete/demote the built-in `admin`, and never allow zero admins.
- **Registration off:** `POST /api/auth/register` returns 403 and the UI hides the register link.
- **Passwords:** argon2 hashes only; generic login errors (no username enumeration); optional login rate-limiting.
- **Passkeys (WebAuthn):** RP ID = the stable Tailscale HTTPS origin (document that changing the hostname invalidates passkeys); enforce **sign-count regression** checks (possible cloned authenticator → reject); allow multiple passkeys and safe deletion (don't let a user delete their only sign-in method if they also have no password).
- **Shared content, private state:** deleting a user cascades all per-user rows; a feed losing its last subscription stops being polled (and may be GC'd) but its items remain only if some user still references them.
- **Shared summary cache:** `item_summaries` is content-derived (keyed by item+model), safe to share; it must contain **no** user-identifying data.
- **Admin-only gating:** ingestion, AI provider, and digest-engine settings/endpoints must reject non-admins (403) at the server, not just hide UI tabs.

**Notifications (ntfy)**
- ntfy secret (`auth_token`) encrypted at rest, never returned/logged; SSRF guard **allows the user-configured ntfy host** (often localhost/LAN) while still validating the URL.
- Feed-health notifications are **throttled**: one per feed per state transition (healthy→failing/disabled), not on every failed poll; de-dupe so a feed shared by many users still notifies each subscriber at most once per transition.
- Post-digest notification only fires if the user enabled it and has a channel; sending failures are logged + surfaced, never crash the digest run. Timeout + one retry.
- Notifications are strictly per-user — never leak another user's counts or feed names.

**Reddit/YouTube** — score/comments only via Reddit JSON (not RSS); `User-Agent` required; honor 429; per-channel RSS not OAuth for polling; handle→channel_id resolution; transcript may be unavailable.

**AI**
- Don't hardcode deprecated model ids; model is per-provider config.
- Provider error / budget exceeded / timeout → raw fallback for digest; clear error for on-demand. Never crash.
- Don't re-summarize cached items; force-refresh only on demand or model change.
- Two API styles only (openai, anthropic); custom endpoints must pick one.

**Filtering/sorting**
- `when` ranges computed in the user's **timezone** (DST-correct), not UTC day boundaries.
- Sort with NULLs last for `top` (score) and `discussed` (comments) so metric-less items don't dominate.
- Category chip counts must reflect the currently-active Type/Status/When facets.
- Pagination + filters are URL-encoded and restored on load.

**Ops & data**
- SQLite: WAL, `busy_timeout`, single-writer pool; ingest inserts in transactions.
- Retention/purge never deletes starred; periodic maintenance task.
- Store UTC, render in user TZ; DST-correct cron.
- Graceful shutdown (flush in-flight writes, close DB) on SIGTERM.
- First-run bootstrap: create DB, run migrations, create the built-in `admin` from `ADMIN_PASSWORD`. Per-user categories (incl. `Other`) are seeded when each account is created; the four example feeds are offered as an optional starter set during onboarding (not force-subscribed).
- Backup: document copying the single SQLite file (`sqlite3 .backup`) + `/api/health`.

---

## 12. Non-functionals
- Full ingest cycle of a typical feed set well under 5 minutes; per-request timeouts enforced.
- Idle memory small enough for a Raspberry Pi 4.
- Structured `tracing` logs, configurable level; errors visible in `docker logs`.
- Config validated at startup with clear errors for missing required env (`SECRET_KEY`, etc.).

---

## 13. Testing
- **Unit:** URL normalization, discovery, dedup/hash, HTML sanitization (XSS payloads stripped), date fallback, backoff schedule, Reddit score/comments parsing + `min_score` filter, `when`-range computation across timezones/DST, sort ordering (incl. NULLs-last), category-required validation + reassignment-to-Other, OPML round-trip, provider-key encryption (and that no endpoint returns a key), argon2 hashing, WebAuthn sign-count regression, admin bootstrap + last-admin guard.
- **Integration:** ingest against **local fixture feeds** (bundled RSS/Atom/JSON/YouTube/Reddit in `/tests/fixtures`) — no live network. Mock both LLM API styles and SMTP. Test items filtering/sorting/pagination end-to-end. **Multi-user isolation:** two users, shared feed polled once, independent read/star/categories; assert no cross-user leakage across every endpoint; admin-only endpoints reject non-admins.
- A "test mode"/seed command that ingests fixtures and prints a sample digest to stdout.

---

## 14. Deliverables (produce all)
1. **Complete repository**, compiling and runnable:
   - Rust backend (`src/`, `migrations/`, `Cargo.toml`).
   - React frontend (`web/`) — mobile-first responsive PWA (manifest + service worker), implements every screen in §9.
   - Tauri v2 Android target wrapping the same frontend → signable `.apk`.
   - `Dockerfile` (multi-stage, multi-arch) + `docker-compose.yml` + `.env.example`.
2. **README** — setup, env vars (incl. required `SECRET_KEY` + `ADMIN_PASSWORD`), run (`docker compose up`), the admin account + user management, registration toggle, passkeys (RP-ID/Tailscale caveat), add/import feeds (OPML), configure categories, AI providers, digest schedule, YouTube import, backup/restore, Tailscale HTTPS (`tailscale cert`/MagicDNS).
3. **Mobile README** — build/sign the Tauri Android `.apk`, install, set API base URL; PWA install instructions.
4. **Architecture doc** (`ARCHITECTURE.md`) — modules, ingestion flow, schema, scheduling, the unified filter/sort model, the **multi-user shared-ingest / per-user-state** model, auth (password + passkeys, roles, admin bootstrap), the pluggable-AI-provider design (two API styles, write-only keys, shared summary cache), single-frontend approach, and key decisions (why SQLite, per-channel RSS, RSS-over-OAuth, PWA+Tauri, categories-not-folders, shared-ingest-vs-duplication).
5. **Example digest output** + **sample `.env`**.
6. **Troubleshooting guide** — feed errors, Reddit 429, provider/AI failures, SQLite locks, PWA/service-worker cache, Android cleartext/HTTPS, and how each is handled/surfaced.

---

## 15. How to modify (document these)
- Manage **users** (admin): change roles, disable/delete, toggle open registration; add/remove **passkeys** (Profile).
- **Admin controls the engine:** ingestion cadence, AI provider/key, digest schedule (all admin-only).
- **Each user sets their own ntfy** channel + which events notify (Settings → Notifications).
- Add/remove feeds and **categories** (UI + OPML); every feed needs a category (`Other` if unsure).
- Add/switch **AI providers** (predefined or custom; set active; delete+recreate to rotate a key).
- Change digest schedule (cron in settings).
- Tune per-feed interval, retention, min-score, concurrency, politeness.
- Adjust default sort / content view / page size.

Build the entire project now. Prioritize: (1) **auth + multi-user foundation** (users/roles, password + passkeys, admin bootstrap, strict per-user scoping) and schema (global feeds/items/AI + per-user subscriptions/state/categories/notifications), (2) shared ingestion engine + edge cases, (3) items API with unified filter/sort/pagination + the card-grid reader/preview UI, (4) pluggable **admin-global** AI (on-demand + digest, shared summary cache), (5) per-user **ntfy notifications** (digest summary + feed-health, test button), (6) Manage/Settings (role-gated tabs)/Profile/Users pages, (7) PWA (manifest + service worker + offline), (8) Docker packaging + docs, (9) Tauri v2 Android. Everything server-side must compile and run via `docker compose up` with only `.env` filled in (`SECRET_KEY` + `ADMIN_PASSWORD`); the web build must be an installable PWA out of the box.
