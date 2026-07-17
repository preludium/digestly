# In-app configuration

This document covers the configuration a self-hoster performs inside the running app - not
environment variables (those are in `README.md`) and not deployment networking (that is in
`docs/deployment.md`).

## Passkeys (WebAuthn)

Any account can add one or more **passkeys** (Touch ID, Windows Hello, a phone, or a hardware
security key) and sign in **passwordless**. Password and passkeys are both valid sign-in methods
for the same account; a passkey is additive, not a replacement.

**Adding and using passkeys:**

- **Add** from **Profile** (or during onboarding): name it, run the browser prompt, done.
  Rename or delete passkeys from the same screen.
- **Sign in** on the login page: type your username, click **Sign in with a passkey**, and
  approve the browser prompt. No password is sent.
- **Discoverable login** (no username required): if the browser and authenticator support
  resident keys, users can sign in without typing a username first.

**Security properties:**

- **Cloned-authenticator protection.** Every assertion's signature counter must move forward; a
  counter that stalls or goes backwards is rejected as a possible clone.
- **You can't lock yourself out.** Digestly refuses to delete your only sign-in method - if you
  have no password, you must keep at least one passkey (or set a password first).

**Relying Party / hostname caveat.** Passkeys are cryptographically bound to `RP_ID` (the
hostname). Set `RP_ID`/`RP_ORIGIN` to your stable production origin (e.g. your Tailscale HTTPS
hostname) and leave them fixed: **changing the hostname permanently invalidates every existing
passkey** (users fall back to their password and re-enrol). WebAuthn also requires a secure
context - it only works over HTTPS, or over `http://localhost` for local dev (the defaults). If
the Relying Party can't be built the feature is disabled and the login button is hidden; the app
stays fully usable with passwords. See `docs/deployment.md` for production `RP_ID`/`RP_ORIGIN`
values.

**Endpoints:**

| Method | Path | Auth |
|--------|------|------|
| `POST` | `/api/auth/passkey/login/options` | public |
| `POST` | `/api/auth/passkey/login/verify` | public |
| `POST` | `/api/auth/passkey/discoverable/login/options` | public |
| `POST` | `/api/auth/passkey/discoverable/login/verify` | public |
| `POST` | `/api/passkeys/register/options` | authed |
| `POST` | `/api/passkeys/register/verify` | authed |
| `GET` | `/api/passkeys` | authed |
| `PATCH` | `/api/passkeys/:id` | authed (rename) |
| `DELETE` | `/api/passkeys/:id` | authed |

---

## AI providers (admin-only)

The admin configures one or more LLM providers in **Settings - AI** and picks the single active
one for the whole instance. Regular users never see AI settings or keys.

**Predefined presets** (base URL + API style baked in - you supply only a key and model):
**Groq**, **OpenAI**, **Anthropic (Claude)**, **Google Gemini**, **Mistral**, and
**Ollama (local)** (no key). Exposed via `GET /api/ai/presets`.

**Custom endpoint**: provide a name, base URL, API style, key, and model.

**Two API styles:**
- `openai` - OpenAI-compatible `POST {base_url}/chat/completions`. Covers Groq, OpenAI, Gemini,
  Mistral, Ollama, and most custom endpoints.
- `anthropic` - `POST {base_url}/messages`.

**Write-only keys.** Keys are submitted once, encrypted at rest with a `SECRET_KEY`-derived key
(ChaCha20-Poly1305), and **never** returned by any endpoint or logged. The UI shows only
"key saved - hidden". To rotate a key, delete the provider and create a new one - there is no
key-edit/read path (`PATCH /api/ai/providers/:id` edits name and model only).

**Test connection** per provider does a minimal live call and reports ok/error without echoing
the key (`POST /api/ai/providers/:id/test`).

**Global AI parameters** (max tokens, temperature, request timeout, daily/monthly token budget)
are also admin-only: `GET /PUT /api/ai/settings`.

**SSRF guard**: custom base URLs are validated against private/loopback ranges. Localhost is
intentionally allowed for Ollama (`provider_type == ollama`).

---

## OPML import / export

From **Manage** (or Settings - Import/Export):

- **Import** an OPML file: each imported feed gets a category (defaulting to `Other` when the
  OPML doesn't specify one), previewed before you confirm.
- **Export** all your subscriptions as OPML.

| Method | Path |
|--------|------|
| `POST` | `/api/opml/import` |
| `GET` | `/api/opml/export` |

---

## Import from YouTube / Reddit (OAuth, optional)

If the admin has configured OAuth credentials (`GOOGLE_OAUTH_*` / `REDDIT_OAUTH_*`),
**Settings - Import / Export - Connected accounts** lets each user link their own YouTube or
Reddit account and pull in the channels / subreddits they follow.

**How it works:**

- **Connect once.** You authorize in the provider's consent screen; Digestly stores only an
  encrypted refresh token (per-user, never returned or logged).
- **Sync now, repeatably.** Fetches your current subscriptions and **adds only the ones you
  don't already have** (already-subscribed feeds are skipped), into a category you pick (default
  `Other`). Reports how many were added vs. skipped.
- **Just RSS underneath.** Imported YouTube channels become per-channel RSS feeds; imported
  subreddits become subreddit feeds - identical to pasting them by hand. Polling never uses
  OAuth; the token is only touched at sync time. **Disconnect** removes the token (imported
  feeds stay).

**OAuth redirect URI.** Register `{RP_ORIGIN}/api/oauth/<provider>/callback` as an authorized
redirect URI in the provider's developer console. See `.env.example` for the exact format.

Providers with no server credentials are **hidden entirely**. The app is fully usable by pasting
channel/subreddit URLs without OAuth.

| Method | Path |
|--------|------|
| `GET` | `/api/oauth/status` |
| `GET` | `/api/oauth/:provider/authorize` |
| `GET` | `/api/oauth/:provider/callback` |
| `POST` | `/api/oauth/:provider/sync` |
| `DELETE` | `/api/oauth/:provider` |

---

## ntfy setup (per-user)

Every user configures their own ntfy channel in **Settings - Notifications** - Digestly does
not bundle or assume a server. Provide:

- `ntfy_server_url` (e.g. `https://ntfy.example.ts.net` or `http://localhost:80`),
- `topic`,
- an optional `auth_token` (write-only, stored encrypted, never returned),
- a `priority`, and
- per-event toggles: **Notify after digest** and **Notify on feed health issues**.

Use the **Test** button (`POST /api/notifications/test`) to send a "Digestly test notification"
to your channel and confirm it works.

Feed-health pushes are throttled to one per feed per healthy-to-failing transition - a feed
shared by many users notifies each subscriber at most once per transition.
