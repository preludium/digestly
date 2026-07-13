# Auth & operations

Authentication, deployment, maintenance, and OAuth import mechanisms.

**Source:** `src/auth/`, `src/oauth/`, `src/config.rs`, `src/maintenance.rs`, `src/healthcheck.rs`, `Dockerfile`, `docker-compose.yml`, `TROUBLESHOOTING.md`

## Authentication

### Passwords (`src/auth/password.rs`)

Passwords are hashed with **argon2** (default params). Plaintext is never stored or logged. Login errors are generic (no username enumeration).

### Sessions (`src/auth/session.rs`)

Sessions use a signed cookie (`hf_session`) plus a revocable `sessions` table:
- Cookie signing key derived from `SECRET_KEY` (SHA-512 → 64-byte `cookie::Key`)
- Sessions survive server restarts
- Revoked on logout, logout-all, or user-delete
- Expiry enforced both client-side (cookie `Max-Age`) and server-side (`expires_at`)

### Passkeys / WebAuthn (`src/auth/passkey.rs`, `src/routes/passkeys.rs`)

Digestly is the WebAuthn Relying Party (`webauthn-rs`):
- Built once at boot from `RP_ID`/`RP_ORIGIN` (with optional `RP_EXTRA_ORIGINS` for dev)
- Held in `AppState` as `Option<Arc<Webauthn>>` — bad config disables the feature without blocking boot
- Ceremony state between `options` and `verify` lives in a short-lived, in-process `CeremonyStore` (never persisted)
- Only the resulting `Passkey` credential is serialized into `passkeys.public_key`
- **Sign-count regression guard:** rejects credentials whose signature counter stalls or goes backwards (cloned authenticator detection)
- **Last-sign-in-method guard:** refuses to delete a user's only credential when they have no password
- Passkeys bind to `RP_ID` — changing the hostname invalidates all existing passkeys

Passwords and passkeys are both valid sign-in methods for the same account.

### Roles

`Role ∈ {admin, user}`. New sign-ups are always `user`. Role gates admin-only screens and endpoints via extractors (`src/auth/extract.rs`):
- `CurrentUser`: resolves the session user; rejects unauthenticated with 401
- `AdminUser`: wraps `CurrentUser` and rejects non-admins with 403

### Admin bootstrap (`src/auth/bootstrap.rs`)

On every boot:
- Ensures the `admin` user exists with the `ADMIN_PASSWORD` hash
- Re-syncs the hash if the env value changed (e.g., password rotation)
- The built-in admin **cannot be deleted or demoted**
- The instance always keeps at least one admin
- Seeds default categories (AI, Software Engineering, Finance, Politics, Lifestyle, Other) for new accounts
- Initializes `app_settings` defaults

### Registration

Open self-signup by default, controlled by `allow_registration` in `app_settings` (admin toggle). When off, the register page/endpoint returns "registration disabled" and only admins can create accounts.

## OAuth imports (`src/oauth/`)

Optional per-user linking of YouTube/Reddit to import followed channels/subreddits as RSS feeds:

- **Client credentials** are instance-level env vars (`GOOGLE_OAUTH_CLIENT_ID`/`_SECRET`, `REDDIT_OAUTH_CLIENT_ID`/`_SECRET`). A provider's feature is hidden unless both are set.
- **Authorization-code flow** stores only an **encrypted refresh token** per user (`user_oauth` table, migration `0002`), never returned or logged
- CSRF `state` binds the callback to the initiating user (in-process `OAuthStates` store)
- "Sync now" is repeatable and idempotent: refreshes an access token, lists subscriptions, maps each to the same feed URL the poller uses, and calls `reconcile` (reuses `feeds::subscribe_url` to add only feeds the user doesn't already have)
- Polling itself is always plain RSS/JSON — the token is used only at sync time

## Deployment

### Docker

Multi-stage `Dockerfile`:
1. **Stage 1 (node):** builds the React frontend (`pnpm build` → `web/dist/`)
2. **Stage 2 (rust):** builds the Rust binary (`cargo build --release`)
3. **Stage 3 (runtime):** `debian:bookworm-slim`, copies the binary and `web/dist/`

`docker-compose.yml` mounts `./data` for the SQLite database and uses `env_file: .env`.

### Environment variables

Env vars are **bootstrap-only** — everything else is configured in the UI and stored in the database.

| Variable | Required | Default | Purpose |
|----------|----------|---------|---------|
| `SECRET_KEY` | **yes** | — | Master secret (≥16 chars). Encrypts provider/ntfy secrets, signs sessions. |
| `ADMIN_PASSWORD` | **yes** | — | Bootstraps and re-syncs the built-in `admin` account. |
| `DATA_DIR` | no | `/data` | Directory holding `digestly.db`. |
| `BIND_ADDR` | no | `0.0.0.0:8080` | HTTP bind address. |
| `STATIC_DIR` | no | `web/dist` | Built frontend assets directory. |
| `RUST_LOG` | no | `info,digestly=debug` | `tracing` filter. |
| `RP_ID` | no | `localhost` | Passkey/WebAuthn Relying Party ID — bare hostname. |
| `RP_ORIGIN` | no | `http://localhost:8080` | Passkey origin (full scheme+host). Must be HTTPS in production. Also the OAuth redirect base. |
| `RP_EXTRA_ORIGINS` | no | — | Comma-separated extra WebAuthn origins (e.g. Vite dev server `http://localhost:5173`). Leave unset in production. |
| `GOOGLE_OAUTH_CLIENT_ID` / `_SECRET` | no | — | Enable **Import from YouTube** (both required). |
| `REDDIT_OAUTH_CLIENT_ID` / `_SECRET` | no | — | Enable **Import from Reddit** (both required). |

The process **fails fast at boot** if `SECRET_KEY` or `ADMIN_PASSWORD` is missing or blank, and rejects a `SECRET_KEY` shorter than 16 characters.

`RP_ID`/`RP_ORIGIN` default to localhost for local dev. **In production set them to your stable Tailscale HTTPS origin.**

### Health check

`docker compose exec digestly /app/digestly --healthcheck` — TCP probe that connects to the server and verifies the DB is alive. Used by Docker HEALTHCHECK.

## Maintenance (`src/maintenance.rs`)

Periodic retention purge (every 6h) driven by `app_settings`:
- `retention.max_age_days` — delete items older than N days
- `retention.max_per_feed` — keep at most M newest items per feed
- Both `0` = keep forever
- **Starred items are never purged** — an item starred by _any_ user survives
- Deletes cascade to `item_states`/`item_summaries` and keep FTS in sync via triggers

## Troubleshooting quick reference

See `TROUBLESHOOTING.md` for detailed scenarios. Common issues:

| Symptom | Resolution |
|---------|------------|
| Feed stops updating | Check Feed Health screen; use retry/re-enable |
| Reddit 429 / no scores | JSON endpoint blocked; falls back to RSS with NULL metrics |
| AI summary/digest fails | Test provider in Settings → AI; check `docker logs` |
| "database is locked" | Don't open the live DB from host while container runs |
| PWA shows stale content | Accept update prompt; or clear site data |
| Android blocks connection | Serve over HTTPS (Tailscale cert) |
| Server won't start | Ensure `SECRET_KEY` (≥16 chars) and `ADMIN_PASSWORD` are set |

## API routes (auth & ops)

- `POST /api/auth/login` — password login
- `POST /api/auth/logout` — session logout
- `POST /api/auth/register` — self-registration
- `GET /api/me` — current user info
- `GET/PUT /api/settings` — per-user preferences
- `POST /api/passkeys/register/options` / `verify` — passkey registration ceremony
- `POST /api/passkeys/login/options` / `verify` — passkey login ceremony
- `GET/DELETE /api/passkeys` — list/delete user's passkeys
- `GET/PATCH/DELETE /api/admin/users` — admin user management
- `GET/PUT /api/admin/users/{id}/role` — role management
- `PUT /api/admin/settings` — admin global settings
- `GET /api/oauth/{provider}/authorize` — OAuth import start
- `GET /api/oauth/{provider}/callback` — OAuth callback
- `POST /api/oauth/{provider}/sync` — OAuth import sync
