# Deployment

Production setup: Tailscale HTTPS, the installed PWA, and backup/restore.

## Tailscale HTTPS

Remote access is out of scope for Digestly itself - reach the server over your own network or
VPN. The intended setup is **Tailscale**: serve Digestly at a tailnet hostname such as
`https://digestly.<tailnet>.ts.net`.

**Why HTTPS is required for the full feature set:**

- The **PWA service worker** only registers in a secure context (HTTPS or `http://localhost`).
  Without it, offline reading and install-to-home-screen do not work.
- **Passkeys (WebAuthn)** require a secure context and bind to `RP_ID` (the bare hostname).
  Changing the hostname after passkeys are enrolled permanently invalidates them.

**Setting up TLS with Tailscale:**

1. Obtain a TLS cert for the tailnet hostname: `tailscale cert digestly.<tailnet>.ts.net`
   (MagicDNS makes this automatic).
2. Set environment variables (see `README.md` for the full env var reference):
   - `RP_ID=digestly.<tailnet>.ts.net`
   - `RP_ORIGIN=https://digestly.<tailnet>.ts.net`
3. Keep `RP_ID`/`RP_ORIGIN` fixed. Changing them after passkeys are enrolled forces every user
   to re-enrol their passkeys (they fall back to password in the meantime).

Digestly does not build any VPN, tunnel, or reverse-proxy logic.

---

## Offline (installed PWA)

Install Digestly to your home screen from a browser on HTTPS to get the full offline experience.

**What works offline:**

- **Reading.** The app shell and any items/content you've already loaded are served from the
  service worker cache. The app opens and reads offline; a banner shows when you're offline.
- **Writing.** Marking items read or starring them offline is applied immediately and queued in
  a small **outbox** (localStorage-backed). When you reconnect - or when the app restarts - the
  queued changes replay to the server. The banner reports how many changes are pending or
  syncing.

**Replay guarantees:**

- Each outbox entry carries an explicit value (not a toggle), so replay is idempotent.
- The outbox coalesces repeated flips per item to your latest intent before replaying, so only
  the last state is sent.
- Where the browser supports the Background Sync API, the service worker replays even after the
  app was closed. Otherwise replay happens on the next reconnect while the app is open.
- The server's read/star endpoints are idempotent upserts, so duplicate replay is safe.

Offline requires the installed PWA over HTTPS. Service workers do not run over plain HTTP except
on `localhost`.

---

## Backup / restore

All state lives in a single SQLite file at `${DATA_DIR}/digestly.db` (default
`/data/digestly.db`, mounted from `./data` by Compose). Backup is a file copy.

**Online-safe backup.** Use SQLite's backup API rather than copying the live file while the
container is writing to it:

```bash
docker compose exec digestly sqlite3 /data/digestly.db ".backup '/data/backup.db'"
# then copy ./data/backup.db off the host
```

**Do not** open the container's live DB directly with `sqlite3` from the host over the Docker
bind mount - cross-boundary WAL locking can corrupt the file. Inspect a running instance through
the API instead. See [`TROUBLESHOOTING.md`](../TROUBLESHOOTING.md).

**Health check.** `GET /api/health` returns `{ "status", "version", "db_ok" }`;
`db_ok: true` means the database is reachable and responding.

**Restore.** Stop the container, replace `./data/digestly.db` with your backup file, restart.
