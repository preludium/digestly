---
name: Task
about: Feature, fix, or improvement with a self-contained implementation plan
title: ""
labels: []
assignees: ""
---

## Why

<!-- The problem or need. What is wrong today, who is affected, and what prompted this. 2-5 sentences. -->

## What

<!-- The desired end state, described from the user's perspective. Not the how - that goes in the implementation plan. -->

## Acceptance criteria

<!-- Checkable, observable outcomes. Each one should be verifiable by driving the real app (or CI), not just by reading code. -->

- [ ] ...

## External behavior this relies on

<!--
Every claim about something we do NOT control - a browser, an OS, a third-party API, a library, a
registry - that this issue's design depends on. One line each, with a link to the documentation
that establishes it. If it cannot be verified from a dev machine, prefix it with ASSUMPTION: so
that reviewers know to challenge it rather than treat it as settled.

Capability claims ("only Chrome supports X", "Y does not work in Z") belong here without exception.
They read like architecture and behave like folklore. An unverified one here silently becomes an
acceptance criterion, and every downstream reader - designer, implementer, reviewer, test - will
defer to it.

Delete this section only if the change touches nothing outside this repo.
-->

- ...

## Scope

<!-- What this issue includes. Files/areas expected to change. -->

## Not in scope

<!-- Explicitly excluded work, adjacent ideas deferred to other issues (link them). -->

## Implementation plan

<!-- Step-by-step plan with exact file paths, function names, and code-level guidance. Should be executable by an agent/developer without further research. Include a Verification subsection with concrete commands and manual checks. -->

### Steps

1. ...

### Verification

- Backend: `cargo test`
- Web: `cd web && pnpm build && pnpm test && pnpm check`
- Manual: ...

## References

<!-- Relevant files (path:line), docs, prior art, related issues/PRs. -->
