# Contributing

## Branching

**Branch name:** `<issue-number>-<short-slug>` (e.g. `7-version-tagged-images`). No issue: drop
the number (`version-tagged-images`). Cut from `main`; never build on a dirty `main`.

## Commit messages

**Format:** [Conventional Commits](https://www.conventionalcommits.org) -
`type(scope): subject`. Lowercase subject, no trailing period. Body explains why, not what.
Close issues with `Closes #N`. Never add an agent as co-author.

**Types used here:** `feat`, `fix`, `ci`, `docs`, `refactor`, `test`, `chore`.

One commit per logical change; keep unrelated files out.

## Pull requests

**Body** follows `.github/pull_request_template.md`. Keep every heading; drop a section only when
it genuinely does not apply. State explicitly what was run and what could NOT be verified locally.
"Should work" is not verification. Merging stays with the repo owner.

## Local setup

```bash
cp .env.example .env
# edit .env: set SECRET_KEY and ADMIN_PASSWORD (both required)
```

**Backend** (terminal 1):

```bash
cd backend
cargo run
```

**Frontend** (terminal 2 - proxies `/api` to the backend at `http://localhost:8080`):

```bash
cd web
pnpm install
pnpm run dev
```

## Seed command (offline fixture test)

Ingest the bundled fixture feeds offline (no network), into a throwaway DB, and print a sample
digest to stdout - useful for testing the ingest + digest pipeline without a live server:

```bash
cd backend
cargo run -- --seed
```

Fixture files are in `backend/tests/fixtures/` (`sample_rss.xml`, `sample_atom.xml`,
`sample_jsonfeed.json`).

## Building the production image

Build the image the same way Compose does:

```bash
docker compose build
```

The multi-stage `Dockerfile` builds web assets, compiles the Rust binary, and copies both into
a `debian-slim` runtime. It builds for the native arch; see
`docs/adr/0003-single-multi-arch-image-via-buildx.md` for cross-arch builds.

## Local checks (mirror CI)

Run these before pushing. They are the exact commands CI runs.

**Backend** (from repo root):

```bash
cargo fmt --manifest-path backend/Cargo.toml --all -- --check
cargo clippy --manifest-path backend/Cargo.toml --all-targets --all-features --locked -- -D warnings
cargo test  --manifest-path backend/Cargo.toml --all-targets --all-features --locked
```

**Frontend** (from `web/`):

```bash
pnpm run ci      # biome ci . - read-only lint + format check
pnpm run build   # tsc --noEmit && vite build
pnpm test        # vitest run
pnpm run e2e     # playwright test (requires a built backend binary)
```

`pnpm run check` (= `biome check --write .`) auto-fixes and can exit 0 when `pnpm run ci` would
fail on the same tree - useful for local cleanup, not the gate.

## CI

CI runs on every PR and push to `main`. All three jobs (`backend`, `frontend`, `e2e`) must be
green before merging. A red job blocks the PR regardless of whether the failure pre-dates the
change. See `docs/adr/0001-ci-is-the-only-merge-gate.md`.
