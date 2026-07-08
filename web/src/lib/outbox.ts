// Offline write-sync outbox (prompt.md §9a — stretch S3). A small persistent queue of read/star
// mutations made while offline (or when a write fails), replayed when connectivity returns.
//
// Design notes:
// * Every queued mutation carries an EXPLICIT value (not a toggle), so replaying it is idempotent
//   — applying `{read:true}` twice is the same as once.
// * Before replay the queue is COALESCED per (kind,itemId) to the latest intent, so a burst of
//   flips offline collapses to one write whose result matches what the user last chose
//   (last-write-wins). This keeps replay conflict-safe against changes made elsewhere meanwhile.
// * The engine is storage-injectable and free of browser/app imports so it is unit-testable in
//   plain Node; the app wires a localStorage-backed store and the real network sender in sync.ts.

export type MutationKind = "read" | "star";

export interface QueuedMutation {
  kind: MutationKind;
  itemId: number;
  value: boolean;
  queuedAt: number;
}

/** Pluggable persistence (localStorage in the app; an in-memory map in tests). */
export interface OutboxStore {
  read(): QueuedMutation[];
  write(list: QueuedMutation[]): void;
}

const STORAGE_KEY = "hf.outbox.v1";

/** Default store backed by `localStorage`; degrades to a no-op if storage is unavailable. */
export const localStorageStore: OutboxStore = {
  read() {
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      return raw ? (JSON.parse(raw) as QueuedMutation[]) : [];
    } catch {
      return [];
    }
  },
  write(list) {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(list));
    } catch {
      /* storage full / disabled — nothing we can do */
    }
  },
};

const keyOf = (m: QueuedMutation) => `${m.kind}:${m.itemId}`;

/** Collapse to the latest mutation per (kind,itemId), ordered by when it was queued. */
export function coalesce(list: QueuedMutation[]): QueuedMutation[] {
  const latest = new Map<string, QueuedMutation>();
  for (const m of list) latest.set(keyOf(m), m); // a later entry overwrites an earlier one
  return [...latest.values()].sort((a, b) => a.queuedAt - b.queuedAt);
}

/** Sends one mutation to the server; throws on failure. */
export type Sender = (m: QueuedMutation) => Promise<void>;
/** Classifies a send error as retryable (offline / server unavailable) vs. terminal. */
export type RetryPredicate = (error: unknown) => boolean;

/**
 * Replay coalesced mutations in order. A retryable failure stops the run and keeps that mutation
 * plus the rest for a later flush; a terminal failure (e.g. the item was deleted) drops it so it
 * can't wedge the queue. Returns the mutations that still need to be persisted.
 */
export async function replay(
  list: QueuedMutation[],
  send: Sender,
  isRetryable: RetryPredicate,
): Promise<QueuedMutation[]> {
  const pending = coalesce(list);
  const remaining: QueuedMutation[] = [];
  let stopped = false;

  for (const m of pending) {
    if (stopped) {
      remaining.push(m);
      continue;
    }
    try {
      await send(m);
    } catch (error) {
      if (isRetryable(error)) {
        stopped = true;
        remaining.push(m);
      }
      // terminal error → drop this mutation and keep going
    }
  }
  return remaining;
}

/** Persistent outbox tying a store to the pure replay engine. */
export class Outbox {
  constructor(private store: OutboxStore = localStorageStore) {}

  enqueue(kind: MutationKind, itemId: number, value: boolean, now: number = Date.now()): void {
    const list = this.store.read();
    list.push({ kind, itemId, value, queuedAt: now });
    this.store.write(list);
  }

  /** Distinct pending mutations (coalesced). */
  pending(): QueuedMutation[] {
    return coalesce(this.store.read());
  }

  count(): number {
    return this.pending().length;
  }

  /** Attempt to send everything; persist whatever couldn't be sent. Returns the remaining count. */
  async flush(send: Sender, isRetryable: RetryPredicate): Promise<number> {
    const remaining = await replay(this.store.read(), send, isRetryable);
    this.store.write(remaining);
    return remaining.length;
  }

  clear(): void {
    this.store.write([]);
  }
}
