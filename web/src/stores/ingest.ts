import { create } from "zustand";

/** Mirrors the server's per-user floor on manual ingestion (`events::COOLDOWN`). Kept in sync
 *  with the server on mount and after every 429, but tracked locally so the button can count
 *  down without polling. */
export const COOLDOWN_MS = 60_000;

interface IngestState {
    /** The user's in-flight "Ingest now" run, or null when nothing is running. */
    runId: number | null;
    /** Feeds polled so far / feeds in the run - the toast's progress line. */
    done: number;
    total: number;
    /** Epoch ms until which "Ingest now" is refused; 0 when it's available. */
    cooldownUntil: number;
    start: (runId: number, total: number) => void;
    progress: (done: number, total: number) => void;
    finish: () => void;
    /** Adopt the server's view (on mount, or after it rejects a run as too soon). */
    sync: (
        run: { run_id: number; done: number; total: number } | null,
        cooldownSecs: number,
    ) => void;
}

/** Ingestion is a background job, not a page: the run outlives the click, survives navigation,
 *  and is watched by the SSE stream mounted at the app root - so its state lives here rather than
 *  in the Feed route or a mutation's `isPending` (which is true for the ~5ms the request takes). */
export const useIngestStore = create<IngestState>((set) => ({
    runId: null,
    done: 0,
    total: 0,
    cooldownUntil: 0,

    start: (runId, total) =>
        set({
            runId,
            done: 0,
            total,
            cooldownUntil: Date.now() + COOLDOWN_MS,
        }),

    progress: (done, total) => set({ done, total }),

    finish: () => set({ runId: null, done: 0, total: 0 }),

    sync: (run, cooldownSecs) =>
        set({
            runId: run?.run_id ?? null,
            done: run?.done ?? 0,
            total: run?.total ?? 0,
            cooldownUntil:
                cooldownSecs > 0 ? Date.now() + cooldownSecs * 1000 : 0,
        }),
}));
