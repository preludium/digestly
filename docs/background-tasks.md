# Background tasks

Digestly runs four long-lived `tokio` tasks. All four are spawned in `backend/src/main.rs` and
all four are aborted in the same graceful-shutdown block. That single file is the place to look
when a task seems missing, doubled up, or leaking on shutdown.

## Where they're wired up

`backend/src/main.rs` is 193 lines. The spawn sites:

| Task             | Spawn call                              | main.rs line |
| ---------------- | --------------------------------------- | ------------ |
| Ingest scheduler | `ingest::spawn(...)`                    | 95           |
| Transcript worker | `ai::transcript_worker::spawn(...)`    | 104-105      |
| Digest scheduler | `digest::spawn(...)`                    | 109          |
| Maintenance      | `maintenance::spawn(...)`               | 112          |

The abort sites, in the same graceful-shutdown block:

```
main.rs:153  scheduler.abort();
main.rs:154  transcript_worker.abort();
main.rs:155  digest_scheduler.abort();
main.rs:156  maintenance.abort();
```

## The tasks

### Ingest scheduler

`backend/src/ingest/scheduler.rs`. Ticks every 15 s (`TICK`, scheduler.rs:27). A
`Semaphore(cfg.concurrency)` caps overall concurrency, with per-host politeness on top. Feeds are
claimed with a 300 s lease (`CLAIM_LEASE_SECS`, scheduler.rs:30) so a slow fetch isn't
re-selected by the next tick, in batches of 50 (`BATCH`, scheduler.rs:33). Each feed's
fetch/parse result is isolated so one bad feed can't stall the rest of the batch.

It's also woken on demand: an `IngestTrigger` (`Arc<Notify>`, created `main.rs:86`) that API
handlers fire via `notify_one()` for refresh-now and new-subscription flows
(`routes/feeds.rs`, `routes/opml.rs`, `routes/settings.rs`). The scheduler awaits it at
scheduler.rs:68.

### Transcript worker

`backend/src/ai/transcript_worker.rs`. Not purely timer-driven: it has a 30 s idle tick
(`TICK`, transcript_worker.rs:29) but is primarily woken by a `TranscriptTrigger`
(`Arc<Notify>`, transcript_worker.rs:26).

The wake path is wired in `main.rs`, not in either task's own module:

- `new_video_trigger` is created at `main.rs:92` as an `Arc<Notify>`.
- It's handed to both the ingest scheduler (via `ingest::spawn`, `main.rs:95-103`) and the
  transcript worker (via `ai::transcript_worker::spawn`, `main.rs:104-105`).
- When the ingest scheduler stores new YouTube items, it calls `new_video_trigger.notify_one()`
  (scheduler.rs:237). The transcript worker awaits it at transcript_worker.rs:51, in a `select!`
  against the 30 s sleep at transcript_worker.rs:52.

Keep the transcript worker's batch small - each transcript fetch is 3 requests to youtube.com
(`BATCH = 10`, transcript_worker.rs:32).

### Digest scheduler

`backend/src/digest/mod.rs` (spawn at mod.rs:608) plus `backend/src/digest/cron.rs`. Uses a
hand-rolled 5-field cron parser (not a crate) that is DST-correct via `chrono-tz`. Also runnable
on demand via `POST /api/digest/run`. See `docs/adr/0004-per-user-incremental-digest-window.md`
for the per-user window decision.

### Maintenance

`backend/src/maintenance.rs`. On startup it runs a one-shot transcript reflow (pure text
transform, no network). After that it sleeps for 6 hours before the first retention purge, then
repeats every 6 hours (`INTERVAL = 6*3600s`, maintenance.rs:13). Starred items always survive
the purge.

## Adding a new background task

**Spawn it in `main.rs` next to the four above, and abort it in the same shutdown block.** Any
task spawned without a matching abort leaks on shutdown.
