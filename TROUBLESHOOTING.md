# Troubleshooting

Each entry is: the symptom, the cause, and how Digestly handles or surfaces it.

## A feed stops updating / shows an error

**Cause:** the source is unreachable, returned an HTTP error, or is malformed.

**How it's handled:** every feed fetch is isolated (one bad feed never affects others). On
failure Digestly increments `failure_count`, stores `last_error`, and reschedules with
exponential backoff + jitter (capped ~6h). Terminal statuses are handled explicitly:

- `410 Gone` → auto-disable with a reason.
- `401` / `403` → auto-disable with a clear error (the feed needs auth).
- `404` → back off, then disable after repeated failures.
- `301` / `308` → follow and persist the new `feed_url`.
- `429` / `Retry-After` → honored.

**Where to look:** the **Feed health** screen (`GET /api/feeds/health`) lists each feed with a
`status` (`ok` / `failing` / `disabled`), `last_error`, `failure_count`, last fetch, and next
retry. Failing/disabled feeds are always surfaced here — never silently dropped. Use *retry
now* / *re-enable* to reset `failure_count` and `disabled` and wake the scheduler.

## Reddit returns 429 / no score or comments

**Cause:** Reddit blocks generic/empty User-Agents and rate-limits aggressively, and its JSON
endpoint often blocks datacenter/cloud IPs entirely.

**How it's handled:** Digestly sends a descriptive `User-Agent` and honors `429`/`Retry-After`
with backoff. It reads metrics (`score`, `num_comments`, `upvote_ratio`) from the JSON
endpoint; if JSON is blocked/unavailable it **falls back to the `.rss` feed** with those
metrics stored as `NULL` and **logs** that the score filter was bypassed (never silent). With
NULL score, the per-feed `min_score` filter is skipped for those items.

**If you need live Reddit metrics** from a blocked IP, run Digestly where Reddit isn't blocking
you, or (future/stretch) add a Reddit OAuth "script" app. Metric parsing itself is correct — it
just needs a reachable JSON endpoint.

## AI summary / digest fails

**Cause:** the provider is misconfigured, unreachable, returns an error, or the token budget is
exceeded.

**How it's handled:**

- **On-demand summarize:** returns a clear error to the UI (nothing is cached).
- **Digest:** the affected category falls back to **raw grouped titles + links** with a
  `fallback_note` in the payload — the digest still generates and archives. It never fails the
  run.
- The token budget guard checks the daily/monthly budget before spending and truncates huge
  source lists.
- `429`/`5xx` are retried with backoff.

**Where to look:** use the **Test** button on a provider (Settings → AI) to do a tiny live call
and get ok/error without echoing the key. Check `docker logs` for the warning that names the
failing category and reason.

## "database is locked" / SQLite lock errors

**Cause:** concurrent writers, or — most commonly in this project — reading the container's live
database from the host over a Docker bind mount.

**How it's handled:** SQLite runs in WAL mode with a `busy_timeout` and a single-writer pool;
ingest inserts run in transactions. Normal container operation does not produce lock errors.

**Important operational note:** do **not** open `${DATA_DIR}/digestly.db` with the host
`sqlite3` CLI while the container is running. Cross-boundary WAL locking over a macOS/Docker
bind mount can produce `disk I/O error` and can corrupt the WAL. Inspect a running instance
**through the API** (`/api/health`, the items/feeds/health endpoints) instead. For a safe
snapshot, run the backup inside the container:

```bash
docker compose exec digestly sqlite3 /data/digestly.db ".backup '/data/backup.db'"
```

## PWA shows stale content / won't update

**Cause:** the service worker is serving cached app-shell assets from a previous version.

**How it's handled / how to force an update:** on a new service worker, an "update available"
prompt appears — accept it to reload with the new version. If it's stuck, clear the site's
storage (browser DevTools → Application → Clear site data / unregister the service worker) and
reload.

**Offline behavior:** offline mode shows **cached items only** and is **read-only** — there is
no offline write queue, so read/star changes made offline are not synced later (that's a
stretch item). When the API is unreachable you get an offline banner and whatever content was
cached.

## Android / PWA blocks the connection (cleartext / no HTTPS)

**Cause:** service workers and Android require a **secure context (HTTPS)**; cleartext HTTP is
blocked by default, so the PWA won't install or register its service worker over plain HTTP.

**How to fix:** serve Digestly over HTTPS at your Tailscale hostname. Obtain a TLS cert with
`tailscale cert` / MagicDNS for `digestly.<tailnet>.ts.net` and front Digestly with it. Once the
origin is HTTPS, the service worker registers and install-to-home-screen / offline reading work.
Digestly does not build any VPN/tunnel/reverse-proxy logic itself.

## Server won't start — missing SECRET_KEY / ADMIN_PASSWORD

**Cause:** a required bootstrap env var is missing or blank.

**How it's handled:** the process **fails fast at boot** with exit code 1 and a clear message
(e.g. `Error: invalid configuration / required environment variable 'SECRET_KEY' is not set`).
`SECRET_KEY` must also be at least 16 characters. Set both in `.env`:

```bash
SECRET_KEY=$(openssl rand -hex 32)
ADMIN_PASSWORD=pick-a-strong-password
```

Then `docker compose up` again.
