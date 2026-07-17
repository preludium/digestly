# Digestly agent instructions

## Commit, push, and PR

**Subagents write to the working tree and stop.** Commit, push, and open a PR only when the
orchestrating layer (the main conversation or a delivery skill) explicitly says to. A subagent
that was not told to commit does not commit.

## Before writing code

Read `docs/code-standards.md`. Read `docs/project-structure.md`. Read `CONTEXT.md` before using
any domain term (Feed, Item, Digest, Ingest, Transcript, Claim, Category).

## Before touching background tasks

Read `docs/background-tasks.md`. All four tasks are spawned and aborted in
`backend/src/main.rs` - that is the only place to look.

## Before claiming anything about external behavior

Read `docs/adr/0002-verify-facts-about-the-outside-world.md`.

## Docs: what goes where

- **`README.md`** - people who want to run Digestly: what it is, quick start, env vars,
  accounts, features, and pointers to the configuration/deployment guides. No deep config
  detail, no CI internals, no contributor workflow.
- **`ARCHITECTURE.md`** - end-to-end narrative: single-service model, ingestion flow, schema,
  auth, AI, frontend. Includes the ADR index (ADRs 0001-0011). Entry point for code navigation.
- **`docs/configuration.md`** - in-app configuration: passkeys, AI providers, OPML, OAuth
  import, ntfy.
- **`docs/deployment.md`** - production networking: Tailscale HTTPS, offline PWA, backup/restore.
- **`CONTRIBUTING.md`** - human contributor flow: branch, commit, PR conventions, local setup,
  local check commands.
- **`CONTEXT.md`** - glossary of domain terms.
- **`docs/`** - maintainer material: CI commands and gate, ADRs, plans, background architecture.
  Extend the doc that already owns the topic before creating a new one.
- **`openwiki/`** - generated. Do not hand-edit; update source code/docs and let OpenWiki
  regenerate.

The OpenWiki scheduled workflow can update this file (`AGENTS.md`) and `openwiki/`. It cannot
update `docs/` or `CONTEXT.md` - that is why substance lives there, not here.
