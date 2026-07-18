# Code standards

## Conventions

These are the standards the codebase is held to. Some are gated by CI; the rest are on the author
and reviewer. The non-gated ones matter just as much - they're just harder to automate.

**One component per `.tsx` file.** One exported component per file. No CI check catches
violations; the reviewer does.

**Keep functions and files short.** If you're scrolling to follow a single function, split it.
`web/biome.json` enables the `recommended` ruleset only - no line-count rules run in CI or
locally.

**Shared before duplicate.** When the same piece exists in two places, consolidate it in the
module that owns the concern. `web/src/components/common/` exists because settings tabs
independently grew their own copies of a tile component; `SettingsTile.tsx` is what that drift
cost. When you reach for copy-paste, check `components/common/` and `src/lib/` first. Not
enforced; caught in review.

**No lint suppressions.** Address the lint; don't silence it. No automated check enforces this.

**`data-testid` only where the DOM is genuinely ambiguous or the copy is volatile.** Prefer
`getByRole`/`getByLabel`/`getByText` - they double as an accessibility check and don't churn when
markup is refactored. Reach for `data-testid` (plus any `data-*` id the element needs, e.g.
`data-item-id`, `data-feed-id`) only when:

- the same content renders more than once in the DOM at once and only CSS visibility tells the
  copies apart (e.g. `Health.tsx`'s mobile `<HealthCard>` list vs. desktop `<HealthRow>` table -
  both mounted regardless of viewport, so a bare `getByText`/`getByRole` throws a Playwright
  strict-mode violation), or
- the only signal an action succeeded is user-facing copy that's expected to change (e.g. `sonner`
  toast text - use `expectToast(page, text)` from `web/e2e/support/api.ts`, which scopes the match
  to `[data-sonner-toast]`, so a copy tweak is a one-line fix there instead of touching every spec
  that asserts it), or
- an element's accessible name is itself the thing under test or is otherwise unstable (e.g. a
  `FilterBar` topic chip's name is `"<category name> <count>"`, so a spec that wants to click a
  specific category should filter on `data-category-id`, not parse the label).

Adding a `data-testid` must not change rendered output, styling, or behavior - it's an inert
attribute. Don't add one where a role/label selector is already unambiguous; three specs quietly
duplicating the same `data-testid` convention is a smaller cost than a spec suite that can't tell
role/label selectors from load-bearing ones.

## What CI enforces

Source of truth: `.github/workflows/ci.yml`.

**Backend** (`backend` job):

| Check              | Command                                                                                         |
| ------------------ | ----------------------------------------------------------------------------------------------- |
| Format             | `cargo fmt --manifest-path backend/Cargo.toml --all -- --check`                                |
| No Clippy warnings | `cargo clippy --manifest-path backend/Cargo.toml --all-targets --all-features --locked -- -D warnings` |
| Tests pass         | `cargo test --manifest-path backend/Cargo.toml --all-targets --all-features --locked`          |

**Frontend** (`frontend` job):

| Check              | Command                                                     |
| ------------------ | ----------------------------------------------------------- |
| Biome lint + format | `pnpm run ci` (= `biome ci .`, read-only, `recommended` preset) |
| TypeScript + build | `pnpm run build` (= `tsc --noEmit && vite build`)           |
| Unit tests         | `pnpm test` (= `vitest run`)                                |

**E2E** (`e2e` job):

- `pnpm run e2e` (= `playwright test`) against a release backend build.

## Local commands

Backend (from repo root):

```bash
cargo fmt --manifest-path backend/Cargo.toml --all -- --check
cargo clippy --manifest-path backend/Cargo.toml --all-targets --all-features --locked -- -D warnings
cargo test  --manifest-path backend/Cargo.toml --all-targets --all-features --locked
```

Frontend (from `web/`):

```bash
pnpm run ci      # biome ci . - read-only, same check CI runs
pnpm run build   # tsc --noEmit && vite build
pnpm test        # vitest run
pnpm run e2e     # playwright test
```

`pnpm run check` (= `biome check --write .`) auto-fixes and can exit 0 even when `pnpm run ci`
would fail on the same tree - useful for local cleanup, not the gate.

## Vendored code

`web/src/components/ui/**` is vendored shadcn, excluded from Biome (`!!src/components/ui` in
`web/biome.json`). Don't edit it - update the vendored source upstream and re-vendor.
