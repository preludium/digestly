# Per-user incremental digest window

**Status: accepted.**

The digest scheduler runs periodically for all users. The original implementation computed each
run's time window as `[now - lookback_days, now]` for every user on every run. When the schedule
fired more than once a day, consecutive runs overlapped: items in the intersection were reprinted,
counted twice in the ntfy summary, and re-sent to the AI provider, doubling token spend. Runs
that produced no new items still archived an empty row and pushed an empty notification.

## Considered options

**De-duplicate items at display time; keep the global window.** Stops the visual reprint but does
not stop re-sending to the AI or double-counting in the push body. Rejected.

**Restrict the schedule to at-most-once-a-day.** Removes the overlap symptom without fixing the
root cause, and constrains admins who have a legitimate need for more frequent runs. Rejected.

## Consequences

`period_end` is computed once per run (shared across all users). `period_start` is computed per
user: for a normal scheduled run it picks up where that user's last digest left off
(`last_digest_end` reads `MAX(period_end)` from the `digests` table), clamped to
`now - lookback_days` for a first-ever digest or after a gap longer than the configured look-back.
A manual run with `lookback_override` forces an explicit `[now - override, now]` window and
ignores the previous-digest boundary entirely.

The code lives in `backend/src/digest/mod.rs`: `compute_period_start`, `last_digest_end`, and the
`start_exclusive` flag passed to `build_and_archive`.

**Lower-bound exclusivity.** On the incremental path (`start_exclusive = true`) the item query
uses `published_at > start` so an item published exactly at the previous boundary second is not
reprinted. First-run, gap, and override paths stay inclusive (`>=`).

**Empty runs are silent.** A run that finds no items for a user produces no archived row and sends
no push. This avoids archiving blank digests when the user has no new content.

**`lookback_days` is now a cap, not a per-run window.** The admin setting describes the maximum
look-back (first digest + gap recovery), not how far back each run scans. The UI help text and
README reflect this.

Closes #13.
