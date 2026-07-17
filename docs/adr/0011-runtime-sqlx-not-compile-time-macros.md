# sqlx runtime query functions over compile-time query macros

**Status: accepted.**

All database queries use `sqlx::query(...)` and `sqlx::query_as(...)` (runtime-checked), not
`sqlx::query!(...)` and `sqlx::query_as!(...)` (compile-time checked). The compile-time macros
require a live database or a checked `sqlx-data.json` file at build time. The multi-stage Docker
build compiles the Rust binary in a stage where no database exists, so the macros would require
either embedding a `sqlx-data.json` in the repository or running a migration step during the
build. Both add friction and a source of drift.

Migrations themselves do use `sqlx::migrate!` (a macro), but this macro only embeds the SQL files
at compile time and runs them at startup - it does not validate queries against a live schema at
build time.

Verified: searching `backend/src/` for `query!` and `query_as!` returns zero results.

## Considered options

**`query!` / `query_as!` macros with a checked `sqlx-data.json`.** The macros provide
compile-time type safety for query results. A `sqlx-data.json` can be generated offline and
committed. Rejected: the file must be regenerated whenever the schema changes; it becomes another
artefact to keep in sync, and forgetting to regenerate it causes misleading build errors.

**`query!` / `query_as!` with a DATABASE_URL available at build time.** Requires a live SQLite
file during `cargo build`. Incompatible with the multi-stage Docker build's compile stage, which
has no database. Rejected.

## Consequences

Query type mismatches are caught at runtime (by tests and early in the application's boot path)
rather than at compile time. The test suite uses a migrated in-memory SQLite pool
(`db::test_pool`) for every backend unit and integration test, so the coverage gap is narrow in
practice. The build requires no `DATABASE_URL` and no `sqlx-data.json`.
