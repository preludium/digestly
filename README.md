# Digestly

A self-hosted, multi-user feed reader with an AI digest, packaged as a single Docker
container. Digestly polls your RSS/Atom/JSON feeds, YouTube channels, and subreddits once,
presents everything as a mobile-first card grid, turns videos into readable text, and pushes
you a scheduled, per-user, LLM-summarized digest over your own ntfy channel.

It runs happily on a Raspberry Pi: one small Rust binary, one SQLite file, no external
database, no Redis, no separate frontend server.

## What it is

- **Multi-user, shared ingest / per-user state.** Feeds are polled _once_ for the whole
  instance no matter how many people subscribe. Everything you consider "yours" - your
  subscriptions, categories, read/star state, notification config, preferences, and digest
  history - is private and scoped to your account. There are no social features and no
  cross-user visibility.
- **Card-grid reader.** A responsive grid (1 column on a phone up to 4 on a wide screen), a
  unified filter bar (type / status / when / sort / category chips), full-text search, and a
  reading preview. No infinite scroll - numbered pagination with the current filter/page
  encoded in the URL.
- **AI summaries + video-as-text.** Summarize any article on demand; video items are
  presented as an AI summary of the transcript (with a collapsible full transcript and a
  deliberately de-emphasized "Watch on YouTube" link) so you read instead of watch. Summaries
  are cached and shared across users, keyed by `(item, model)`.
- **Per-user ntfy notifications.** Each user points Digestly at their own ntfy server/topic
  and gets a push after each digest run and when one of their feeds starts failing.
- **Scheduled AI digests.** A single admin-owned cron schedule drives per-user, category-
  grouped digests summarized by the active AI provider and archived to each user's history.
- **OPML import/export** and an installable **PWA**: read cached items offline, and read/star
  offline too - those changes queue and sync when you reconnect.

## Tech stack

| Layer       | Choices                                                                          |
| ----------- | -------------------------------------------------------------------------------- |
| Backend     | Rust, `axum` (HTTP), `tokio` (async), `sqlx` over SQLite (WAL + FTS5)            |
| Parsing     | `feed-rs` (RSS 2.0 / RSS 1.0-RDF / Atom / JSON Feed), `ammonia` (HTML sanitize)  |
| HTTP client | `reqwest` (rustls, gzip/brotli, cookies off)                                     |
| Crypto      | `argon2` (passwords), ChaCha20-Poly1305 (secrets at rest, key from `SECRET_KEY`) |
| Frontend    | React + TypeScript + Vite, TanStack Query, Zustand, Tailwind, React Router       |
| Packaging   | One multi-stage Docker image (build web → build Rust → slim runtime)             |

The Rust binary serves the built React SPA itself (`tower-http::ServeDir` + SPA fallback), so
there is exactly one deployable service on one port with one SQLite file.

## Quick start

```bash
cp .env.example .env
```

Set the **two required secrets** in `.env`:

```bash
# a long random master secret (encrypts stored secrets + signs sessions)
SECRET_KEY=$(openssl rand -hex 32)

# the password for the built-in `admin` account
ADMIN_PASSWORD=pick-a-strong-password
```

Then bring it up:

```bash
docker compose up
```

Open <http://localhost:8080> and log in as **`admin`** with the `ADMIN_PASSWORD` you set.
The image runs on both ARM64 (Raspberry Pi) and x86-64.

> The `admin` account is bootstrapped on first boot from `ADMIN_PASSWORD`. If you change
> `ADMIN_PASSWORD` later, the hash is re-synced on the next boot.

## Environment variables

Env vars are **bootstrap-only** - everything else is configured in the UI and stored in the
database. Only these are read (see `backend/src/config.rs`):

| Variable                             | Required | Default                 | Purpose                                                                                                                                                                           |
| ------------------------------------ | -------- | ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `SECRET_KEY`                         | **yes**  | -                       | Master secret (≥16 chars). Encrypts provider/ntfy secrets, signs sessions.                                                                                                        |
| `ADMIN_PASSWORD`                     | **yes**  | -                       | Bootstraps and re-syncs the built-in `admin` account.                                                                                                                             |
| `DATA_DIR`                           | no       | `/data`                 | Directory holding `digestly.db`. Compose pins this to the mounted volume.                                                                                                         |
| `BIND_ADDR`                          | no       | `0.0.0.0:8080`          | Address the HTTP server binds to.                                                                                                                                                 |
| `STATIC_DIR`                         | no       | `../web/dist`           | Directory of built frontend assets (image sets `/app/static`).                                                                                                                    |
| `RUST_LOG`                           | no       | `info,digestly=debug`   | `tracing` filter; logs are visible in `docker logs`.                                                                                                                              |
| `RP_ID`                              | no       | `localhost`             | Passkey/WebAuthn Relying Party ID - the bare hostname passkeys bind to.                                                                                                           |
| `RP_ORIGIN`                          | no       | `http://localhost:8080` | Passkey origin (full scheme+host). Must be HTTPS in production. Also the OAuth redirect base.                                                                                     |
| `RP_EXTRA_ORIGINS`                   | no       | -                       | Comma-separated extra WebAuthn origins to also accept (e.g. local Vite dev server `http://localhost:5173`). Leave unset in production - only `RP_ORIGIN` should be trusted there. |
| `GOOGLE_OAUTH_CLIENT_ID` / `_SECRET` | no       | -                       | Enable **Import from YouTube** (both required).                                                                                                                                   |
| `REDDIT_OAUTH_CLIENT_ID` / `_SECRET` | no       | -                       | Enable **Import from Reddit** (both required).                                                                                                                                    |

The process **fails fast at boot** with a clear message if `SECRET_KEY` or `ADMIN_PASSWORD`
is missing or blank, and rejects a `SECRET_KEY` shorter than 16 characters.

`RP_ID`/`RP_ORIGIN` default to localhost so passkeys work in local dev. **In production set them
to your stable Tailscale HTTPS origin** (see [Passkeys](#passkeys-webauthn) and
[Tailscale HTTPS](#tailscale-https)). If they are unparseable the app still boots - passkeys are
simply disabled and the login button is hidden.

## Accounts, roles, and registration

- **Roles are `admin` or `user`.** New sign-ups are always `user`. Roles gate the admin-only
  screens and endpoints server-side, not just in the UI.
- **The built-in `admin`** cannot be deleted or demoted, and the instance always keeps at
  least one admin (last-admin guard).
- **Open registration** is on by default and controlled by the admin-owned setting
  `allow_registration`. Turn it off (Users screen) and the register page/endpoint returns a
  clear "registration disabled" state; only the admin can then create accounts.
- **Admins manage accounts only** - list users, change roles, disable/enable, delete (which
  cascades all of that user's data), and toggle open registration. Admins never see another
  user's feed contents.
- **Ingestion, AI providers, and the digest engine are admin-only, instance-wide** settings.
  Every user configures their own feeds, categories, preferences, and ntfy notifications.

## Passkeys (WebAuthn)

Any account can add one or more **passkeys** (Touch ID, Windows Hello, a phone, or a hardware
security key) and sign in **passwordless**. Password and passkeys are both valid sign-in methods
for the same account, so a passkey is additive - not a replacement.

- **Add a passkey** from **Profile** (or during onboarding): name it, run the browser prompt,
  done. Rename or delete passkeys from the same screen.
- **Sign in** on the login page: type your username, click **Sign in with a passkey**, and
  approve the browser prompt. No password is sent.
- **Cloned-authenticator protection.** Every assertion's signature counter must move forward; a
  counter that stalls or goes backwards is rejected as a possible clone.
- **You can't lock yourself out.** Digestly refuses to delete your _only_ sign-in method - if
  you have no password, you must keep at least one passkey (or set a password first).

**Relying Party / hostname caveat.** Passkeys are cryptographically bound to `RP_ID` (the
hostname). Set `RP_ID`/`RP_ORIGIN` to your stable **Tailscale HTTPS** origin and leave them
fixed: **changing the hostname permanently invalidates every existing passkey** (users fall back
to their password and re-enrol). WebAuthn also requires a secure context - it only works over
HTTPS, or over `http://localhost` for local dev (the defaults). If the RP can't be built the
feature is disabled and the login button is hidden; the app stays fully usable with passwords.

Endpoints: `POST /api/auth/passkey/login/{options,verify}` (public),
`POST /api/passkeys/register/{options,verify}`, `GET /api/passkeys`,
`PATCH /api/passkeys/{id}` (rename), `DELETE /api/passkeys/{id}` (authed).

## Adding feeds

Use **Add feed** and paste any of the following into the single discovery input:

- a site URL (Digestly sniffs `<link rel="alternate">` and common paths like `/feed`,
  `/rss`, `/atom.xml`, `/feed.json`),
- a direct feed URL,
- a YouTube channel URL, `@handle`, or `channel_id` (polled via per-channel RSS -
  `https://www.youtube.com/feeds/videos.xml?channel_id=<ID>` - no OAuth, no quota),
- a subreddit (e.g. `r/programming`).

Every subscription **must** have a category - the Add-feed dialog blocks submitting without
one and lets you create a category inline. Reddit subscriptions expose a `min_score` floor;
advanced options (fetch interval, full-text extraction) are collapsed.

### How often feeds are checked

New feeds are checked **once a day** by default. Digestly is built around a daily digest, and
polling harder than that mostly buys rate-limiting: YouTube soft-blocks bursty pollers (see
`docs/youtube-feed-throttling.md`) and Reddit throttles anonymous requests aggressively.

Change it in two places:

- **Per feed** - the fetch interval under a subscription's advanced options.
- **Instance-wide default** - Settings → Ingestion → *Default check interval* (admin), which
  applies to feeds added from then on.

> **Upgrading from a version before the daily default?** The default was 1 hour. Existing feeds
> keep whatever interval they were created with - only newly added feeds pick up the new default,
> and only if an admin never set *Default check interval* explicitly (an explicit setting always
> wins).

### OPML import / export

From **Manage** (or Settings → Import/Export):

- **Import** an OPML file; each imported feed gets a category (defaulting to `Other` when the
  OPML doesn't specify one), previewed before you confirm.
- **Export** all your subscriptions as OPML.

Endpoints: `POST /api/opml/import`, `GET /api/opml/export`.

### Import from YouTube / Reddit (OAuth, optional)

If the admin has configured OAuth credentials (`GOOGLE_OAUTH_*` / `REDDIT_OAUTH_*`), **Settings →
Import / Export → Connected accounts** lets each user link their own YouTube or Reddit account and
pull in the channels / subreddits they follow:

- **Connect once.** You authorize in the provider's consent screen; Digestly stores only an
  encrypted refresh token (per-user, never returned or logged).
- **Sync now, repeatably.** Press it whenever you like - it fetches your current subscriptions and
  **adds only the ones you don't already have** (already-subscribed feeds are skipped), into a
  category you pick (default `Other`). It reports how many were added vs. skipped.
- **Just RSS underneath.** Imported YouTube channels become per-channel RSS feeds and imported
  subreddits become subreddit feeds - identical to pasting them by hand. Polling never uses OAuth;
  the token is only touched at sync time. **Disconnect** removes the token (imported feeds stay).

Providers with no server credentials are **hidden entirely**, and the app is fully usable by
pasting channel/subreddit URLs. The OAuth redirect URI is `{RP_ORIGIN}/api/oauth/<provider>/callback`

- register that exact URL in the provider console (see `.env.example`).

Endpoints: `GET /api/oauth/status`, `GET /api/oauth/{provider}/authorize`,
`GET /api/oauth/{provider}/callback`, `POST /api/oauth/{provider}/sync`,
`DELETE /api/oauth/{provider}`.

## Categories

Categories are the single grouping concept - there are no folders. Every subscription belongs
to exactly one, mandatory category, and categories are also the buckets the digest groups by.

Each account is seeded with six categories on creation: **AI**, **Software Engineering**,
**Finance**, **Politics**, **Lifestyle**, and **Other**. `Other` is the non-deletable
catch-all: deleting any other category reassigns its feeds to `Other`.

## AI providers (admin-only)

The admin configures one or more LLM providers in **Settings → AI** and picks the single
active one for the whole instance. Regular users never see AI settings or keys.

- **Predefined presets** (base URL + API style baked in - you supply only a key and model):
  **Groq**, **OpenAI**, **Anthropic (Claude)**, **Google Gemini**, **Mistral**, and
  **Ollama (local)** (no key). Exposed via `GET /api/ai/presets`.
- **Custom endpoint**: provide a name, base URL, API style, key, and model.
- **Two API styles only:** `openai` (OpenAI-compatible `POST {base_url}/chat/completions` -
  covers Groq/OpenAI/Gemini/Mistral/Ollama/most custom) and `anthropic`
  (`POST {base_url}/messages`).
- **Write-only keys.** Keys are submitted once, encrypted at rest with a `SECRET_KEY`-derived
  key, and **never** returned by any endpoint or logged. The UI shows only "key saved ·
  hidden". **To rotate a key, delete the provider and create a new one** - there is no
  key-edit/read path (PATCH edits name/model only).
- **Test connection** per provider does a tiny live call and reports ok/error without echoing
  the key.

Global AI parameters (max tokens, temperature, request timeout, daily/monthly token budget)
are also admin-only.

## Digest schedule

The digest **engine is admin-configured and instance-wide** (Settings → Digest): enable/
disable, a cron schedule (default daily, 09:00), a maximum look-back window (default 24h - used
for a user's first digest and as a fallback after a long gap; otherwise each run picks up where
that user's last digest left off), a timezone, which categories to include, and whether to use
AI. The schedule preview renders human-readable ("Every day at 09:00 (UTC)").

**Content is per-user.** Each run iterates users and builds each one a digest of _their_ own
subscriptions grouped by _their_ categories, with one AI prompt per non-empty category via the
active provider. If the provider is unavailable or the token budget is exceeded, the digest
still generates using raw grouped titles with a fallback note - it never fails the run. Each
digest is archived to that user's history and, if they enabled it, pushed to their ntfy
channel. Admins can also **Run digest now** (`POST /api/digest/run`).

See [`docs/example-digest.md`](docs/example-digest.md) for the rendered shape and the ntfy
push text.

## ntfy setup (per-user)

Every user configures their own ntfy channel in **Settings → Notifications** - Digestly does
not bundle or assume a server. Provide:

- `ntfy_server_url` (e.g. `https://ntfy.example.ts.net` or `http://localhost:80`),
- `topic`,
- an optional `auth_token` (write-only, stored encrypted, never returned),
- a `priority`, and
- per-event toggles: **Notify after digest** and **Notify on feed health issues**.

Use the **Test** button (`POST /api/notifications/test`) to send a "Digestly test
notification" to your channel and confirm it works. Feed-health pushes are throttled to one
per feed per healthy→failing transition.

## Backup / restore

All state lives in a single SQLite file at `${DATA_DIR}/digestly.db` (default `/data/digestly.db`,
mounted from `./data` by compose). Backup is a file copy.

For a consistent online backup, use SQLite's backup API rather than copying the live file
while the container writes to it:

```bash
docker compose exec digestly sqlite3 /data/digestly.db ".backup '/data/backup.db'"
# then copy ./data/backup.db off the host
```

`GET /api/health` reports `{ "status", "version", "db_ok" }`; `db_ok: true` means the database
is reachable.

> **Do not** open the container's live DB with `sqlite3` from the host over the Docker bind
> mount - cross-boundary WAL locking can corrupt the file. Inspect a running instance through
> the API instead. See [`TROUBLESHOOTING.md`](TROUBLESHOOTING.md).

## Tailscale HTTPS

Remote access is out of scope - reach the server over your own network/VPN. The intended
setup is **Tailscale**: serve Digestly at a tailnet hostname (e.g.
`https://digestly.<tailnet>.ts.net`).

The **PWA and its service worker require a secure context (HTTPS)**. Obtain a TLS cert for the
tailnet hostname with `tailscale cert` / MagicDNS and serve Digestly over HTTPS at that
hostname so the service worker registers and offline reading / install-to-home-screen work.
Digestly itself does not build any VPN/tunnel/reverse-proxy logic.

**Passkeys** need the same HTTPS origin. Set `RP_ID` to the tailnet hostname
(`digestly.<tailnet>.ts.net`) and `RP_ORIGIN` to `https://digestly.<tailnet>.ts.net`, then keep
them fixed - changing the hostname invalidates enrolled passkeys (see [Passkeys](#passkeys-webauthn)).

## Offline (installed PWA)

Once installed, Digestly keeps working without a connection:

- **Reading** - the app shell and any items/content you've already loaded are served from cache,
  so the app opens and reads offline. A banner shows when you're offline.
- **Writing** - marking items read or starring them offline is applied immediately and added to a
  small **outbox**. When you reconnect (or reopen the app) the queued changes replay to the server;
  the banner reports how many are pending / syncing. Each change is stored with its explicit value
  and the outbox coalesces repeated flips per item to your latest choice, so replay is idempotent
  and conflict-safe (last-write-wins). Where the browser supports the Background Sync API the
  service worker replays even after the app was closed; otherwise replay happens on the next
  reconnect while the app is open.

Offline requires the installed PWA over HTTPS (service workers don't run over plain HTTP except on
`localhost`).

## Not yet built (stretch / future)

These appear in the spec but are **not** implemented in the current build; they are documented
as future work, not present features:

- **Tauri v2 Android app** - the same React build is designed to power it, but no Android
  target is built yet.
- **Admin aggregate delivery** (email/webhook/file) - per-user ntfy is the delivery path.

## Development

The frontend dev server proxies `/api` to the backend, so run both:

```bash
# terminal 1 - backend (reads .env; point DATA_DIR at a writable dir like ./data)
cd backend
cargo run

# terminal 2 - frontend with hot reload; Vite proxies /api → http://localhost:8080
cd web
npm install
npm run dev
```

Run the tests:

```bash
cd backend
cargo test
```

Test-mode seed command - ingests the bundled `backend/tests/fixtures/*` feeds offline (no network)
into a throwaway DB and prints a sample digest to stdout:

```bash
cd backend
cargo run -- --seed
```

To build the production image the same way compose does:

```bash
docker compose build
```

The multi-stage `Dockerfile` builds the web assets, compiles the Rust binary, and copies both
into a `debian-slim` runtime. It builds for ARM64 and x86-64.
