## Delivering work

Code changes ship as a pull request, without being asked. Default flow for any task that
touches the repo:

1. Work in a git worktree off `main` (isolates the change; never build on a dirty `main`).
2. Branch named `<type>/<short-slug>` matching the commit type, e.g. `ci/version-tagged-images`.
3. Commit with [Conventional Commits](https://www.conventionalcommits.org): `type(scope): subject`
   in the imperative, lowercase subject, no trailing period. Types used here: `feat`, `fix`,
   `ci`, `docs`, `refactor`, `test`, `chore`. Body explains why, not what. Close issues with
   `Closes #N`. Never add an agent as co-author.
4. Open a PR (`gh pr create`) describing what changed, any behavior change callers must know
   about, and how it was verified - including what could NOT be verified locally.

One commit per logical change; keep unrelated files out of it. Push and PR are the end of the
task, not a separate ask. Merging stays with the user.

## Docs: what goes where

- `README.md` is for **people who want to run Digestly**: what it is, quick start, configuration,
  features, operations they perform themselves. No CI internals, no build plumbing, no
  contributor workflow. If a change only matters to someone hacking on the repo, it does not
  belong here.
- `docs/` holds technical/maintainer material - CI, release process, build architecture, design
  plans. Extend the existing doc that owns the topic (e.g. `docs/multi-arch.md` owns Docker
  image builds and release tags) before creating a new one.
- `openwiki/` is generated - see below.

Before adding prose anywhere, ask which audience it serves; that answers which file it lands in.

## OpenWiki

This repository uses OpenWiki for recurring code documentation. Start with `openwiki/quickstart.md`, then follow its links to architecture, workflows, domain concepts, operations, integrations, testing guidance, and source maps.

The scheduled OpenWiki GitHub Actions workflow refreshes the repository wiki. Do not hand-edit generated OpenWiki pages unless explicitly asked; prefer updating source code/docs and letting OpenWiki regenerate.
