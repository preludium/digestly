## Repo conventions

When branches, commits, or PRs are created for this repo, they follow:

- **Branch:** `<issue-number>-<short-slug>`, e.g. `7-version-tagged-images`. With no issue, drop the number: `version-tagged-images`. Cut from `main`; never build on a dirty `main`.
- **Commit message:** [Conventional Commits](https://www.conventionalcommits.org) - `type(scope): subject` in imperative, lowercase subject, no trailing period. Types used here: `feat`, `fix`, `ci`, `docs`, `refactor`, `test`, `chore`. Body explains why, not what. Close issues with `Closes #N`. Never add an agent as co-author.
- **PR body** follows `.github/pull_request_template.md`: What (+`Closes #N`), Why, Behavior changes, Verification, Screenshots for UI, Follow-ups. Keep the headings; drop a section only when it truly does not apply. Verification states what was actually run and what could NOT be verified locally - never "should work".
- One commit per logical change; keep unrelated files out. Merging stays with the user.

Whether to commit, push, or open a PR is a decision for the orchestrating layer (the main conversation, a delivery skill like `orchestrated-feature`), not for individual subagents. Subagents write to the working tree and stop there unless their prompt tells them otherwise.

## Facts about the outside world

Claim about how something *we do not control* behaves - browser, OS, third-party API, library, registry - is not design decision. It is fact: either verified, or guess wearing fact's clothes.

- **Verify before it becomes premise.** Any such claim a spec/issue/plan leans on gets checked against real docs (`context7`, vendor docs) *before* becoming acceptance criterion. Cite source in issue.
- **Unverifiable ones get labelled.** Cannot check from here (needs device, paid account, real phone)? Write `ASSUMPTION:` in front. That word is licence for every downstream reader to challenge it. Unmarked claim gets treated as settled by everyone who reads it - that is the whole failure mode.
- **Never ask agent for "research-free" design.** That instruction converts "I don't know" into confident assertion, which then propagates with authority of spec.
- **Reviewers challenge premises, not just conformance.** "Code correctly implements false premise" is a finding, ranked by what it costs user - not shrug because issue said so. Review dispatch must say this explicitly; otherwise reviewer defers to spec by design, and wrong spec sails through.
- **Green tests written from premise cannot test that premise.** They restate it. Passing suite is evidence about code, never about world.

Capability claims are sharp edge: "only X supports Y", "Z doesn't work on W". Read like architecture, behave like folklore. Check them.

## Docs: what goes where

- `README.md` is for **people who want to run Digestly**: what it is, quick start, configuration, features, operations they perform themselves. No CI internals, no build plumbing, no contributor workflow. If change only matters to someone hacking on repo, it does not belong here.
- `docs/` holds technical/maintainer material - CI, release process, build architecture, design plans. Extend existing doc that owns topic (e.g. `docs/multi-arch.md` owns Docker image builds and release tags) before creating new one.
- `openwiki/` is generated - see below.

Before adding prose anywhere, ask which audience it serves; that answers which file it lands in.

## OpenWiki

This repository uses OpenWiki for recurring code documentation. Start with `openwiki/quickstart.md`, then follow its links to architecture, workflows, domain concepts, operations, integrations, testing guidance, and source maps.

Scheduled OpenWiki GitHub Actions workflow refreshes repository wiki. Do not hand-edit generated OpenWiki pages unless explicitly asked; prefer updating source code/docs and letting OpenWiki regenerate.
