// Offline write-sync wiring (prompt.md §9a - stretch S3). Bridges the pure outbox engine to the
// real network, TanStack Query, and the browser's connectivity signals.
//
// A read/star mutation is applied optimistically to the cache, then sent. If the send fails while
// offline (or the server is unreachable) it's queued; the queue is flushed on the `online` event,
// on a Background-Sync message from the service worker, and once on app start.

import { type QueryClient, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { ApiError, api } from "@/lib/api";
import { type MutationKind, Outbox, type QueuedMutation } from "@/lib/outbox";
import { requestOutboxSync } from "@/lib/pwa";

/** The app-wide outbox (localStorage-backed by default). */
export const outbox = new Outbox();

const ITEMS_KEY = ["items"];
const COUNTS_KEY = ["categories", "counts"];

/** A failure is retryable if we're offline or the server is unreachable / unavailable. */
export function isRetryable(error: unknown): boolean {
    if (typeof navigator !== "undefined" && !navigator.onLine) return true;
    return (
        error instanceof ApiError &&
        (error.status === 0 || error.status === 503)
    );
}

/** Send one queued mutation to its per-user endpoint (always an explicit value → idempotent). */
async function send(m: QueuedMutation): Promise<void> {
    await api.post(`/items/${m.itemId}/${m.kind}`, { value: m.value });
}

/**
 * Apply a read/star change: optimistic cache patch, then send. On a retryable failure the mutation
 * is queued and a Background Sync is requested; the optimistic value stays so the UI is consistent
 * offline. Returns the value that was applied.
 */
export async function sendOrQueue(
    kind: MutationKind,
    itemId: number,
    value: boolean,
): Promise<boolean> {
    try {
        await send({ kind, itemId, value, queuedAt: 0 });
        return value;
    } catch (error) {
        if (!isRetryable(error)) throw error;
        outbox.enqueue(kind, itemId, value);
        requestOutboxSync();
        notifyOutboxChanged();
        return value;
    }
}

/** Flush the queue; invalidate item/count queries if anything drained so the UI reconciles. */
export async function flushOutbox(qc?: QueryClient): Promise<void> {
    const before = outbox.count();
    if (before === 0) return;
    const remaining = await outbox.flush(send, isRetryable);
    notifyOutboxChanged();
    if (remaining < before && qc) {
        qc.invalidateQueries({ queryKey: ITEMS_KEY });
        qc.invalidateQueries({ queryKey: COUNTS_KEY });
    }
}

const OUTBOX_CHANGED = "hf-outbox-changed";
function notifyOutboxChanged() {
    if (typeof window !== "undefined")
        window.dispatchEvent(new CustomEvent(OUTBOX_CHANGED));
}

/**
 * Drive outbox flushing and expose the pending count for the UI. Flushes on reconnect, on a
 * service-worker `hf-flush-outbox` message (Background Sync), and once on mount (app reopened with
 * a queue). Mount this once, high in the tree.
 */
export function useOutboxSync(): number {
    const qc = useQueryClient();
    const [count, setCount] = useState(() => outbox.count());

    useEffect(() => {
        const refresh = () => setCount(outbox.count());
        const flush = () => void flushOutbox(qc).finally(refresh);
        const onSwMessage = (e: MessageEvent) => {
            if (e.data?.type === "hf-flush-outbox") flush();
        };

        window.addEventListener("online", flush);
        window.addEventListener(OUTBOX_CHANGED, refresh);
        navigator.serviceWorker?.addEventListener("message", onSwMessage);

        // App may have reopened online with a queue left from a previous session.
        flush();

        return () => {
            window.removeEventListener("online", flush);
            window.removeEventListener(OUTBOX_CHANGED, refresh);
            navigator.serviceWorker?.removeEventListener(
                "message",
                onSwMessage,
            );
        };
    }, [qc]);

    return count;
}
