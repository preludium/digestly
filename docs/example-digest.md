# Example digest

A rendered digest is built per user from the archived `payload_json` (see
`src/digest/mod.rs`) and displayed on the digest detail screen (`GET /api/digest/{id}`,
`src/routes/digest.rs`). This page shows both variants - with AI summaries and with the
raw-titles fallback - plus the ntfy push text that accompanies a run.

The payload groups items by the user's categories (in category order), one section per
non-empty category, with a `sources` list and optional notes (`ai_used`, `fallback_note`,
`failure_warning`).

---

## Variant 1 - with AI summaries

> # Digest - 2026-07-08
>
> Period: 2026-07-01 → 2026-07-08 · 26 new articles
>
> ## AI (14)
>
> - Open-weights momentum continued: two labs shipped mixture-of-experts models that beat
>   last quarter's dense baselines on reasoning benchmarks while running on a single GPU.
> - Retrieval-augmented agents are the recurring theme - several posts converge on smaller
>   models plus good retrieval outperforming larger models used naively.
> - Tooling maturity: eval harnesses and cost/latency dashboards are becoming standard in
>   write-ups, signalling a shift from demos to production concerns.
>
> Sources: r/MachineLearning, Import AI, The Batch
>
> - [MoE models close the gap on single-GPU reasoning](https://example.com/moe-single-gpu) - r/MachineLearning
> - [A practical guide to RAG for agents](https://example.com/rag-agents) - Import AI
>
> ## Software Engineering (12)
>
> - Post-incident writeups dominated: two widely-shared retros trace outages to config
>   rollouts without staged canaries, reinforcing gradual-rollout tooling.
> - Rust and Go both shipped releases focused on build times and diagnostics rather than new
>   language surface.
> - SQLite-as-application-database keeps gaining advocates for small-to-medium services,
>   echoing this app's own architecture.
>
> Sources: Hacker News, r/programming, r/softwareengineering
>
> - [What a staged rollout would have caught](https://example.com/staged-rollout) - Hacker News
> - [Why we moved back to SQLite](https://example.com/back-to-sqlite) - r/programming
>
> **Sources:** Hacker News, r/programming, r/softwareengineering, r/MachineLearning, Import AI, The Batch

Here `ai_used` is `true` and each section's `raw` flag is `false` - the bullets come from the
active AI provider.

---

## Variant 2 - raw-titles fallback (provider unavailable)

When there is no active provider, or a provider call fails, or the token budget is exceeded,
the affected sections fall back to raw grouped titles + links and the digest carries a
`fallback_note`. The run still completes and archives - it never fails.

> # Digest - 2026-07-08
>
> Period: 2026-07-01 → 2026-07-08 · 26 new articles
>
> > **Note:** AI summaries were unavailable (no active provider) - showing raw titles.
>
> ## AI (14)
>
> - [MoE models close the gap on single-GPU reasoning](https://example.com/moe-single-gpu) - r/MachineLearning
> - [A practical guide to RAG for agents](https://example.com/rag-agents) - Import AI
> - [Weekly paper roundup](https://example.com/paper-roundup) - The Batch
>
> ## Software Engineering (12)
>
> - [What a staged rollout would have caught](https://example.com/staged-rollout) - Hacker News
> - [Why we moved back to SQLite](https://example.com/back-to-sqlite) - r/programming
> - [Diagnostics improvements in the latest release](https://example.com/diagnostics) - r/softwareengineering
>
> **Sources:** Hacker News, r/programming, r/softwareengineering, r/MachineLearning, Import AI, The Batch

If more than two of the user's sources failed to fetch in the window, a `failure_warning` is
also shown (and included in the push), e.g. _"3 of your sources failed to fetch recently."_

---

## ntfy push text

After a digest run, if the user enabled digest pushes and has a channel configured, Digestly
sends a summary notification (`digest_push_body` in `src/digest/mod.rs`):

```
Title: Digestly digest
Body:  26 new articles across AI (14), Software Engineering (12)
```

When there is a fetch-failure warning it is appended:

```
Title: Digestly digest
Body:  26 new articles across AI (14), Software Engineering (12)
       ⚠ 3 of your sources failed to fetch recently.
```

A user with no ntfy channel still gets the digest generated and archived (viewable in the app);
it just isn't pushed.
