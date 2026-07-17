<!-- Keep the headings; delete any section that genuinely does not apply. -->

## What

<!-- What this PR changes, from the perspective of someone reading the diff cold. Lead with the outcome. -->

Closes #

## Why

<!-- The problem this solves. If the issue already says it, one sentence and a link is enough. -->

## Behavior changes

<!-- Anything a user, deployer, or caller must do differently, or would notice. Say "None" if there are none. -->

None.

## Verification

<!--
What you actually ran, with real output/results - not "should work". State explicitly what could
NOT be verified locally and why.

CI already runs `cargo fmt`/`clippy`/`cargo test`, `pnpm run ci`/`build`/`test`, and e2e on every
PR - don't re-assert those here. This checklist is for what CI can't check.

A green suite is evidence about the code, never about the world: tests written from a premise
restate that premise, they do not test it. So if this PR relies on external behavior (browser, OS,
third-party API, library), link the doc that establishes it - see the issue's "External behavior
this relies on".
-->

- [ ] Exercised the change in the running app:
- [ ] External behavior this relies on is verified against docs (or marked ASSUMPTION in the issue):

Not verified locally:

## Screenshots

<!-- UI changes only: before / after, including a mobile width. Delete this section otherwise. -->

## Follow-ups

<!-- Known gaps deliberately left out of scope, with issue links where they exist. Delete if none. -->
