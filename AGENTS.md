## Delivering work

Code changes ship as pull request, without being asked. Default flow for any task that touches repo:

1. Work in git worktree off `main` (isolates change; never build on dirty `main`).
2. Branch named `<issue-number>-<short-slug>`, e.g. `7-version-tagged-images`. With no issue, drop number: `version-tagged-images`.
3. Commit with [Conventional Commits](https://www.conventionalcommits.org): `type(scope): subject` in imperative, lowercase subject, no trailing period. Types used here: `feat`, `fix`, `ci`, `docs`, `refactor`, `test`, `chore`. Body explains why, not what. Close issues with `Closes #N`. Never add agent as co-author.
4. Open PR (`gh pr create`) whose body follows `.github/pull_request_template.md`: What (+`Closes #N`), Why, Behavior changes, Verification, Screenshots for UI, Follow-ups. Keep headings; drop section only when it truly does not apply. Verification states what was actually run and what could NOT be verified locally - never "should work".

One commit per logical change; keep unrelated files out. Push and PR are end of task, not separate ask. Merging stays with user.

## Docs: what goes where

- `README.md` is for **people who want to run Digestly**: what it is, quick start, configuration, features, operations they perform themselves. No CI internals, no build plumbing, no contributor workflow. If change only matters to someone hacking on repo, it does not belong here.
- `docs/` holds technical/maintainer material - CI, release process, build architecture, design plans. Extend existing doc that owns topic (e.g. `docs/multi-arch.md` owns Docker image builds and release tags) before creating new one.
- `openwiki/` is generated - see below.

Before adding prose anywhere, ask which audience it serves; that answers which file it lands in.

## OpenWiki

This repository uses OpenWiki for recurring code documentation. Start with `openwiki/quickstart.md`, then follow its links to architecture, workflows, domain concepts, operations, integrations, testing guidance, and source maps.

Scheduled OpenWiki GitHub Actions workflow refreshes repository wiki. Do not hand-edit generated OpenWiki pages unless explicitly asked; prefer updating source code/docs and letting OpenWiki regenerate.
