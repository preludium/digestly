# Verify facts about the outside world before they become premises

**Status: accepted.**

Plans, issues, and specs regularly contain claims about things we do not control: browser APIs,
OS behavior, third-party endpoints, library semantics, registry behavior. When a claim is wrong,
it propagates through design, tests, and review without being caught - everyone downstream treats
it as settled. Green tests written from a bad premise restate the premise; they do not test the
world.

## Considered options

**Trust the plan as written; verify only when something breaks.** Faster to start. Fails because
a wrong premise survives design, implementation, and review intact, and surfaces only in
production or user-reported failure.

**Flag only claims that seem risky.** Ambiguous by definition. What one person treats as
established fact another treats as uncertain. A uniform rule is easier to apply and harder to
skip.

## Consequences

**Verify before it becomes a premise.** Any claim a spec or plan leans on gets checked against
real docs (context7, vendor docs) before it becomes an acceptance criterion. Cite the source in
the issue.

**Unverifiable claims get labelled.** Cannot check from here (needs a device, paid account, real
phone, third-party access)? Write `ASSUMPTION:` in front. That word licenses every downstream
reader to challenge it. An unmarked claim gets treated as settled by everyone who reads it - that
is the failure mode.

**Never ask for research-free design.** That instruction converts "I don't know" into a confident
assertion, which propagates with the authority of a spec.

**Reviewers challenge premises, not just conformance.** "Code correctly implements a false
premise" is a finding, ranked by the cost to the user - not a shrug because the issue said so.
Review dispatch must say this explicitly; otherwise the reviewer defers to the spec by design.

**Capability claims are a sharp edge.** "Only X supports Y", "Z doesn't work on W" - these read
like architecture decisions and behave like folklore. Check them.
