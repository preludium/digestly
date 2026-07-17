# SQLite over Postgres and Redis

**Status: accepted.**

Digestly targets a home server or Raspberry Pi: one container, one file to back up, low idle RAM.
A multi-process setup (Postgres + Redis + the app) would add memory overhead and operational
complexity that is simply inappropriate for that deployment context. SQLite in WAL mode with
FTS5 covers all the workload this application needs without an external database.

The pool is configured in `backend/src/db.rs`: WAL journal mode, `busy_timeout` of 5 s (so brief
write contention does not surface as an error), foreign keys on, and a pool capped at 5
connections. SQLite is single-writer; under WAL, readers and the single writer do not block each
other, so the small pool is sufficient.

## Considered options

**Postgres.** Correct choice for multi-instance horizontal scale or complex analytics. Requires a
separate process, persistent volume, and connection management. Rejected: the workload is a single
instance; no multi-writer concurrency is needed; the ops cost is disproportionate.

**Redis (for sessions or caching).** Sessions are rows in the `sessions` table; the summary cache
is `item_summaries`. Both fit naturally in SQLite. Adding Redis for these would introduce a second
persistence dependency. Rejected.

## Consequences

The database is a single file at `${DATA_DIR}/digestly.db`. Backup is a file copy (or
`sqlite3 .backup` for an online-safe copy). WAL + FTS5 allow full-text search across titles and
content without an external search index. The application can only run as a single instance
(single-writer constraint); horizontal scaling is out of scope.
