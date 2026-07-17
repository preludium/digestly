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

The tech stack is Rust + axum + SQLite (backend) and React + Vite + Tailwind (frontend), shipped
as a single multi-stage Docker image. See [ARCHITECTURE.md](ARCHITECTURE.md) for the full stack
breakdown, module map, and key design decisions.

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
to your stable Tailscale HTTPS origin** (see [docs/deployment.md](docs/deployment.md) and
[docs/configuration.md](docs/configuration.md#passkeys-webauthn)). If they are unparseable the
app still boots - passkeys are simply disabled and the login button is hidden.

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

Any account can add passkeys (Touch ID, Windows Hello, a hardware key) and sign in
passwordless. Passkeys are additive - password login remains available.

Passkeys are cryptographically bound to `RP_ID`. In production, set `RP_ID`/`RP_ORIGIN` to
your stable HTTPS hostname before enrolling passkeys; changing the hostname later invalidates
all existing ones. See [docs/configuration.md](docs/configuration.md#passkeys-webauthn) for
the full detail (cloned-authenticator protection, lockout guard, endpoints).

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

From **Manage** (or Settings → Import/Export) you can import an OPML file (each feed gets a
category, previewed before you confirm) and export all your subscriptions. See
[docs/configuration.md](docs/configuration.md#opml-import--export) for the detail and endpoints.

### Import from YouTube / Reddit (OAuth, optional)

If the admin has configured `GOOGLE_OAUTH_*` / `REDDIT_OAUTH_*` credentials, **Settings →
Import / Export → Connected accounts** lets each user link their account and pull in the
channels or subreddits they follow. Sync is repeatable and idempotent; polling always uses plain
RSS, never OAuth. See [docs/configuration.md](docs/configuration.md#import-from-youtube--reddit-oauth-optional)
for the full detail and endpoint list.

## Categories

Categories are the single grouping concept - there are no folders. Every subscription belongs
to exactly one, mandatory category, and categories are also the buckets the digest groups by.

A new account starts with a single category, **Other** - the non-deletable catch-all:
deleting any other category reassigns its feeds to `Other`. Everything else is created as you
go: opting into the starter feeds during onboarding adds the categories those feeds use
(**Software Engineering**, **AI**), and you can create your own at any time.

## AI providers (admin-only)

The admin configures one or more LLM providers in **Settings → AI** and picks the single
active one for the whole instance. Predefined presets cover Groq, OpenAI, Anthropic, Gemini,
Mistral, and Ollama; custom endpoints are also supported. Keys are write-only - encrypted at
rest, never returned. See [docs/configuration.md](docs/configuration.md#ai-providers-admin-only)
for API styles, key rotation, test-connection, global AI parameters, and endpoints.

## Digest schedule

The digest **engine is admin-configured and instance-wide** (Settings → Digest): enable/
disable, a cron schedule (default daily, 05:00 UTC), a maximum look-back window (default 24h -
used for a user's first digest and as a fallback after a long gap; otherwise each run picks up
where that user's last digest left off), a timezone, which categories to include, and whether to
use AI. The schedule preview renders human-readable ("Every day at 05:00 (UTC)").

**Content is per-user.** Each run builds each user a digest of their own subscriptions grouped
by their categories, with one AI prompt per non-empty category via the active provider. If the
provider is unavailable or the token budget is exceeded, the digest still generates using raw
grouped titles with a fallback note - it never fails the run. Each digest is archived to that
user's history and, if they enabled it, pushed to their ntfy channel. Admins can also
**Run digest now** (`POST /api/digest/run`).

See [`docs/example-digest.md`](docs/example-digest.md) for the rendered shape and the ntfy
push text.

## ntfy setup (per-user)

Every user configures their own ntfy channel in **Settings → Notifications** - Digestly does
not bundle or assume a server. Provide a server URL, topic, optional auth token, priority, and
per-event toggles (digest / feed health). See
[docs/configuration.md](docs/configuration.md#ntfy-setup-per-user) for the full field list and
the test-notification endpoint.

## Running in production

Set `RP_ID`/`RP_ORIGIN` to your stable HTTPS hostname, obtain a TLS cert (Tailscale
`tailscale cert` works well), and back up the single SQLite file at `${DATA_DIR}/digestly.db`.
See [docs/deployment.md](docs/deployment.md) for Tailscale HTTPS setup, the offline PWA,
and backup/restore instructions.

## Documentation

| Document | Contents |
|----------|----------|
| [ARCHITECTURE.md](ARCHITECTURE.md) | Tech stack, module map, ingestion/digest/auth flow, key decisions (ADRs) |
| [docs/configuration.md](docs/configuration.md) | Passkeys, AI providers, OPML, OAuth import, ntfy |
| [docs/deployment.md](docs/deployment.md) | Tailscale HTTPS, offline PWA, backup/restore |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Branch/commit/PR conventions, local dev setup, CI |
| [CONTEXT.md](CONTEXT.md) | Domain glossary (Feed, Item, Digest, Ingest, ...) |
| [TROUBLESHOOTING.md](TROUBLESHOOTING.md) | Common problems and fixes |
