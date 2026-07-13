import { describe, expect, it } from "vitest";
import {
    coalesce,
    Outbox,
    type OutboxStore,
    type QueuedMutation,
    replay,
} from "./outbox";

/** In-memory store for deterministic tests. */
function memoryStore(): OutboxStore {
    let list: QueuedMutation[] = [];
    return {
        read: () => list.map((m) => ({ ...m })),
        write: (l) => {
            list = l.map((m) => ({ ...m }));
        },
    };
}

describe("coalesce", () => {
    it("keeps only the latest mutation per (kind,item), ordered by queue time", () => {
        const list: QueuedMutation[] = [
            { kind: "read", itemId: 1, value: true, queuedAt: 1 },
            { kind: "star", itemId: 1, value: true, queuedAt: 2 },
            { kind: "read", itemId: 1, value: false, queuedAt: 3 }, // supersedes the first read
            { kind: "read", itemId: 2, value: true, queuedAt: 4 },
        ];
        const out = coalesce(list);
        expect(out).toEqual([
            { kind: "star", itemId: 1, value: true, queuedAt: 2 },
            { kind: "read", itemId: 1, value: false, queuedAt: 3 },
            { kind: "read", itemId: 2, value: true, queuedAt: 4 },
        ]);
    });
});

describe("replay", () => {
    const retryable = (e: unknown) =>
        e instanceof Error && e.message === "offline";

    it("sends coalesced mutations and clears the queue on success", async () => {
        const sent: QueuedMutation[] = [];
        const list: QueuedMutation[] = [
            { kind: "read", itemId: 1, value: true, queuedAt: 1 },
            { kind: "read", itemId: 1, value: false, queuedAt: 2 },
            { kind: "star", itemId: 5, value: true, queuedAt: 3 },
        ];
        const remaining = await replay(
            list,
            async (m) => void sent.push(m),
            retryable,
        );
        // Only the final read intent + the star are sent.
        expect(sent).toEqual([
            { kind: "read", itemId: 1, value: false, queuedAt: 2 },
            { kind: "star", itemId: 5, value: true, queuedAt: 3 },
        ]);
        expect(remaining).toEqual([]);
    });

    it("stops at a retryable failure and keeps that mutation and the rest", async () => {
        const list: QueuedMutation[] = [
            { kind: "read", itemId: 1, value: true, queuedAt: 1 },
            { kind: "star", itemId: 2, value: true, queuedAt: 2 },
            { kind: "read", itemId: 3, value: true, queuedAt: 3 },
        ];
        const remaining = await replay(
            list,
            async (m) => {
                if (m.itemId === 2) throw new Error("offline");
            },
            retryable,
        );
        // item 1 sent; item 2 failed (retryable) → kept; item 3 never attempted → kept.
        expect(remaining.map((m) => m.itemId)).toEqual([2, 3]);
    });

    it("drops a terminal failure so it can't wedge the queue", async () => {
        const list: QueuedMutation[] = [
            { kind: "read", itemId: 1, value: true, queuedAt: 1 }, // 404 - item gone
            { kind: "star", itemId: 2, value: true, queuedAt: 2 },
        ];
        const sent: number[] = [];
        const remaining = await replay(
            list,
            async (m) => {
                if (m.itemId === 1) throw new Error("gone");
                sent.push(m.itemId);
            },
            retryable,
        );
        expect(sent).toEqual([2]);
        expect(remaining).toEqual([]);
    });
});

describe("Outbox", () => {
    it("persists across instances and reports a coalesced count", () => {
        const store = memoryStore();
        const a = new Outbox(store);
        a.enqueue("read", 1, true, 1);
        a.enqueue("read", 1, false, 2); // same item → coalesces
        a.enqueue("star", 1, true, 3);
        expect(a.count()).toBe(2);

        // A fresh instance sees the same persisted queue.
        const b = new Outbox(store);
        expect(b.count()).toBe(2);
    });

    it("flush drains the queue and returns remaining count", async () => {
        const store = memoryStore();
        const box = new Outbox(store);
        box.enqueue("read", 1, true, 1);
        box.enqueue("star", 2, true, 2);

        let attempts = 0;
        const remaining = await box.flush(
            async (m) => {
                attempts++;
                if (m.itemId === 2) throw new Error("offline");
            },
            (e) => e instanceof Error && e.message === "offline",
        );
        expect(attempts).toBe(2);
        expect(remaining).toBe(1); // the star stays queued
        expect(box.count()).toBe(1);
        expect(box.pending()[0].itemId).toBe(2);
    });
});
