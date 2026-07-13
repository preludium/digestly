//! Live server→browser events, and the ingest "runs" they report on.
//!
//! `POST /api/feeds/refresh-all` only marks the user's feeds due and pokes the scheduler; the
//! actual polling happens later in the background loop, so the HTTP response cannot tell the
//! client when *their* ingestion finished. A run bridges that gap: the API opens one over the
//! user's feed ids, the scheduler decrements it as each feed is polled, and the browser watches
//! progress + completion over the SSE stream (`GET /api/events`).
//!
//! Runs live in memory on purpose. They are ephemeral, this is a single process, and a run lost
//! to a restart is recoverable on the client (it falls back to a timeout + refetch), so persisting
//! them would buy nothing and cost a table plus writes on every poll.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::broadcast;
use tracing::{debug, warn};

/// Manual "Ingest now" is a deliberate burst across every feed the user has, and bursty polling is
/// exactly what gets an instance soft-blocked by YouTube/Reddit. The button has a floor.
pub const COOLDOWN: Duration = Duration::from_secs(60);

/// A run whose feeds never all report back is force-finished, so a stuck feed can't strand the
/// client's toast forever. Reachable when a feed is already in flight under an earlier tick's
/// claim lease, or is disabled between the refresh and the poll.
const RUN_TTL: Duration = Duration::from_secs(300);

/// Lagging receivers drop events rather than slow the scheduler down. Survivable: the client
/// refetches its items on `ingest_finished` regardless of how many `feed_polled` it missed, and
/// a client that misses the finish falls back to its own timeout.
const CHANNEL_CAP: usize = 256;

pub type RunId = u64;
pub type Events = Arc<EventBus>;

/// What one feed poll produced. Handed back to the registry by the scheduler.
#[derive(Clone, Copy, Debug, Default)]
pub struct PollOutcome {
    pub new_items: usize,
    pub failed: bool,
}

impl PollOutcome {
    pub fn stored(new_items: usize) -> Self {
        Self {
            new_items,
            failed: false,
        }
    }

    pub fn unchanged() -> Self {
        Self::default()
    }

    pub fn failed() -> Self {
        Self {
            new_items: 0,
            failed: true,
        }
    }
}

/// Everything the browser can be told about. Serialized as the SSE `data` payload; the client
/// switches on `type`.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// One feed of a run finished polling - drives the toast's "7 of 12 sources" progress.
    FeedPolled {
        run_id: RunId,
        done: usize,
        total: usize,
    },
    /// Every feed of a run reported back (or the run hit its TTL). The client refetches here.
    IngestFinished {
        run_id: RunId,
        new_items: usize,
        failed: usize,
        timed_out: bool,
    },
}

/// An event addressed to exactly one user. The SSE handler filters the broadcast on `user_id`;
/// `feeds` rows are shared between users, so events must never be.
#[derive(Clone, Debug)]
pub struct UserEvent {
    pub user_id: i64,
    pub event: Event,
}

/// A snapshot of the caller's in-flight run, for restoring button/toast state after a reload.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct RunStatus {
    pub run_id: RunId,
    pub done: usize,
    pub total: usize,
}

pub struct EventBus {
    tx: broadcast::Sender<UserEvent>,
    state: Mutex<State>,
    next_id: AtomicU64,
}

#[derive(Default)]
struct State {
    runs: HashMap<RunId, Run>,
    last_run_at: HashMap<i64, Instant>,
}

struct Run {
    user_id: i64,
    pending: HashSet<i64>,
    total: usize,
    new_items: usize,
    failed: usize,
    started: Instant,
}

impl EventBus {
    pub fn new() -> Events {
        let (tx, _rx) = broadcast::channel(CHANNEL_CAP);
        Arc::new(Self {
            tx,
            state: Mutex::new(State::default()),
            next_id: AtomicU64::new(1),
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<UserEvent> {
        self.tx.subscribe()
    }

    /// Open a run over `feed_ids`. `Err(secs)` when the user is still inside the ingest cooldown.
    pub fn open_run(&self, user_id: i64, feed_ids: Vec<i64>) -> Result<RunId, u64> {
        let mut state = self.lock();
        let left = cooldown_left(&state, user_id);
        if left > 0 {
            return Err(left);
        }
        state.last_run_at.insert(user_id, Instant::now());

        let run_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let pending: HashSet<i64> = feed_ids.into_iter().collect();
        let total = pending.len();
        state.runs.insert(
            run_id,
            Run {
                user_id,
                pending,
                total,
                new_items: 0,
                failed: 0,
                started: Instant::now(),
            },
        );
        debug!(run_id, user_id, feeds = total, "ingest run opened");
        Ok(run_id)
    }

    /// Report a finished feed poll. Decrements *every* open run waiting on this feed: `feeds` are
    /// shared, so two users who both hit "Ingest now" on the same subreddit are both satisfied by
    /// the single poll that follows.
    pub fn feed_done(&self, feed_id: i64, outcome: PollOutcome) {
        let mut out = Vec::new();
        {
            let mut state = self.lock();
            let mut finished = Vec::new();

            for (&run_id, run) in state.runs.iter_mut() {
                if !run.pending.remove(&feed_id) {
                    continue;
                }
                run.new_items += outcome.new_items;
                if outcome.failed {
                    run.failed += 1;
                }
                out.push(UserEvent {
                    user_id: run.user_id,
                    event: Event::FeedPolled {
                        run_id,
                        done: run.total - run.pending.len(),
                        total: run.total,
                    },
                });
                if run.pending.is_empty() {
                    finished.push(run_id);
                }
            }

            for run_id in finished {
                if let Some(run) = state.runs.remove(&run_id) {
                    debug!(run_id, new_items = run.new_items, "ingest run finished");
                    out.push(UserEvent {
                        user_id: run.user_id,
                        event: Event::IngestFinished {
                            run_id,
                            new_items: run.new_items,
                            failed: run.failed,
                            timed_out: false,
                        },
                    });
                }
            }
        }
        self.emit(out);
    }

    /// Force-finish runs past their TTL. Called on every scheduler tick.
    pub fn sweep(&self) {
        let mut out = Vec::new();
        {
            let mut state = self.lock();
            let stale: Vec<RunId> = state
                .runs
                .iter()
                .filter(|(_, run)| run.started.elapsed() > RUN_TTL)
                .map(|(&id, _)| id)
                .collect();

            for run_id in stale {
                let Some(run) = state.runs.remove(&run_id) else {
                    continue;
                };
                warn!(
                    run_id,
                    pending = run.pending.len(),
                    "ingest run timed out - force-finishing"
                );
                out.push(UserEvent {
                    user_id: run.user_id,
                    event: Event::IngestFinished {
                        run_id,
                        new_items: run.new_items,
                        failed: run.failed,
                        timed_out: true,
                    },
                });
            }
        }
        self.emit(out);
    }

    /// The caller's in-flight run, if any. A user has at most one: the cooldown outlasts the run
    /// far more often than not, and a second run would only re-poll feeds the first is polling.
    pub fn run_for(&self, user_id: i64) -> Option<RunStatus> {
        let state = self.lock();
        state
            .runs
            .iter()
            .find(|(_, run)| run.user_id == user_id)
            .map(|(&run_id, run)| RunStatus {
                run_id,
                done: run.total - run.pending.len(),
                total: run.total,
            })
    }

    /// Seconds until this user may ingest again; 0 when they may ingest now.
    pub fn cooldown_left(&self, user_id: i64) -> u64 {
        cooldown_left(&self.lock(), user_id)
    }

    fn emit(&self, events: Vec<UserEvent>) {
        for ev in events {
            // No receivers (nobody has the app open) is the normal case, not an error.
            let _ = self.tx.send(ev);
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // A panic while holding this lock would leave the registry consistent (every mutation is
        // a single map update), so recovering beats poisoning the whole ingest pipeline.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn cooldown_left(state: &State, user_id: i64) -> u64 {
    let Some(last) = state.last_run_at.get(&user_id) else {
        return 0;
    };
    COOLDOWN
        .checked_sub(last.elapsed())
        .map(|d| d.as_secs() + 1) // round up: 0 means "go", so never report 0 while blocking
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(rx: &mut broadcast::Receiver<UserEvent>) -> Vec<UserEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            out.push(ev);
        }
        out
    }

    #[test]
    fn run_finishes_when_every_feed_reports() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let run = bus.open_run(1, vec![10, 11]).unwrap();

        bus.feed_done(10, PollOutcome::stored(3));
        bus.feed_done(11, PollOutcome::stored(4));

        let events = drain(&mut rx);
        assert!(matches!(
            events.last().map(|e| &e.event),
            Some(Event::IngestFinished {
                run_id,
                new_items: 7,
                failed: 0,
                timed_out: false,
            }) if *run_id == run
        ));
        assert!(bus.run_for(1).is_none());
    }

    #[test]
    fn one_poll_of_a_shared_feed_satisfies_every_run() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        bus.open_run(1, vec![10]).unwrap();
        bus.open_run(2, vec![10]).unwrap();

        bus.feed_done(10, PollOutcome::stored(2));

        // Sorted: runs live in a HashMap, so which user is notified first is not defined - only
        // that both are.
        let mut finished: Vec<i64> = drain(&mut rx)
            .into_iter()
            .filter(|e| matches!(e.event, Event::IngestFinished { .. }))
            .map(|e| e.user_id)
            .collect();
        finished.sort_unstable();
        assert_eq!(finished, vec![1, 2]);
    }

    #[test]
    fn a_failed_feed_still_completes_the_run() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        bus.open_run(1, vec![10]).unwrap();

        bus.feed_done(10, PollOutcome::failed());

        assert!(matches!(
            drain(&mut rx).last().map(|e| &e.event),
            Some(Event::IngestFinished { failed: 1, .. })
        ));
    }

    #[test]
    fn cooldown_blocks_a_second_run() {
        let bus = EventBus::new();
        bus.open_run(1, vec![10]).unwrap();

        let left = bus.open_run(1, vec![10]).unwrap_err();
        assert!(left > 0 && left <= COOLDOWN.as_secs() + 1);
        // Scoped to the user, not the instance.
        assert!(bus.open_run(2, vec![10]).is_ok());
    }

    #[test]
    fn an_unrelated_feed_leaves_the_run_alone() {
        let bus = EventBus::new();
        bus.open_run(1, vec![10]).unwrap();

        bus.feed_done(99, PollOutcome::stored(5));

        let run = bus.run_for(1).expect("run still open");
        assert_eq!((run.done, run.total), (0, 1));
    }
}
