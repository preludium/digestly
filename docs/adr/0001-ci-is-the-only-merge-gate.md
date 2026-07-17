# CI is the only merge gate

**Status: accepted.**

The project runs three CI jobs on every PR and push to `main` (see `.github/workflows/ci.yml`):
`backend` (fmt, clippy, tests), `frontend` (biome, build, vitest), and `e2e` (Playwright against a
release build). These are the only required pre-merge gates.

## Considered options

**Manual review as the primary gate.** Reviewer judgment is inconsistent across people and sessions;
machine-checkable errors slip through. Rejected because it makes quality enforcement depend on
attention rather than automation.

**Additional mandatory gates (performance benchmarks, visual regression).** Nothing in this
category is expressible as a CI check today. Any such gate would block merges on
not-yet-implemented infrastructure. Deferred until there is a real implementation.

## Consequences

Every PR must have all three jobs green before merging. A red job blocks the PR regardless of
whether the failure pre-dates the change - pre-existing failures are not exempt.

The canonical CI commands are in `docs/code-standards.md` alongside the matching local commands, so
authors can reproduce the gate before pushing.
