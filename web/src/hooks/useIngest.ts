import type { QueryClient } from "@tanstack/react-query";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useRef } from "react";
import { toast } from "sonner";
import { ApiError, api } from "@/lib/api";
import type { IngestStatus, ServerEvent } from "@/lib/types";
import { useIngestStore } from "@/stores/ingest";

/** A run whose `ingest_finished` never arrives (server restarted, stream wedged behind a proxy)
 *  must not leave the toast spinning. Comfortably longer than the server's own run TTL for the
 *  common case, short enough that a wedged stream doesn't hold the button hostage. */
const RUN_TIMEOUT_MS = 120_000;

/** Derived from the run id, so the same toast can be updated in place from anywhere - and
 *  reopened after a reload without having stashed a handle. */
const toastId = (runId: number) => `ingest-${runId}`;

/** Everything the ingest touches: the item list, the unread/category counts, and feed health
 *  (a poll can flip a feed to failing). Awaited, so "complete" never lands before the items do. */
function refreshFeedData(qc: QueryClient) {
    return Promise.all([
        qc.invalidateQueries({ queryKey: ["items"] }),
        qc.invalidateQueries({ queryKey: ["categories"] }),
        qc.invalidateQueries({ queryKey: ["feeds", "health"] }),
    ]);
}

/** "Ingest now" (§9.0). Marks the user's feeds due and opens a run; the run's *completion* comes
 *  back over the SSE stream, since the request itself returns before the scheduler has polled
 *  anything. */
export function useIngestNow() {
    const start = useIngestStore((s) => s.start);
    const sync = useIngestStore((s) => s.sync);

    return useMutation({
        mutationFn: () =>
            api.post<{ ok: boolean; run_id: number | null; feeds: number }>(
                "/feeds/refresh-all",
            ),
        onSuccess: ({ run_id, feeds }) => {
            if (run_id === null) {
                toast("Nothing to ingest - you have no active feeds");
                return;
            }
            start(run_id, feeds);
            toast.loading("Ingesting your feeds…", {
                id: toastId(run_id),
                description: `0 of ${feeds} sources polled`,
                duration: Number.POSITIVE_INFINITY,
            });
        },
        onError: async (e) => {
            toast.error(
                e instanceof Error ? e.message : "Could not start ingestion",
            );
            // Rejected by the cooldown: our clock disagrees with the server's, so take theirs.
            if (e instanceof ApiError && e.status === 429) {
                const status = await api
                    .get<IngestStatus>("/ingest/status")
                    .catch(() => null);
                if (status) sync(status.run, status.cooldown_secs);
            }
        },
    });
}

/** The app's single live connection to the server. Mounted once, at the app shell.
 *
 *  Owns the ingest toast end-to-end: it opens on the click, tracks each feed as it's polled, and
 *  resolves only after the refetched items are on screen - so "Ingest complete" and the new items
 *  appear together rather than the toast promising something the list hasn't shown yet. */
// biome-ignore lint/complexity/noExcessiveLinesPerFunction: existing baseline
export function useIngestEvents() {
    const qc = useQueryClient();
    const { runId, sync, progress, finish } = useIngestStore();

    const settle = useCallback(
        async (id: number, message: string, ok: boolean) => {
            await refreshFeedData(qc);
            finish();
            const show = ok ? toast.success : toast.error;
            show(message, {
                id: toastId(id),
                // Updating a toast in place keeps whatever fields we don't overwrite, so the
                // progress line has to be cleared explicitly - otherwise the finished toast still
                // reads "12 of 12 sources polled" underneath its own summary.
                description: undefined,
                duration: 5000,
            });
        },
        [qc, finish],
    );

    // Restore state on mount: a reload mid-run (or a second tab) picks the run back up instead of
    // pretending nothing is happening. Re-opens the toast under the same derived id.
    useEffect(() => {
        let cancelled = false;
        api.get<IngestStatus>("/ingest/status")
            .then((status) => {
                if (cancelled) return;
                sync(status.run, status.cooldown_secs);
                if (status.run) {
                    toast.loading("Ingesting your feeds…", {
                        id: toastId(status.run.run_id),
                        description: `${status.run.done} of ${status.run.total} sources polled`,
                        duration: Number.POSITIVE_INFINITY,
                    });
                }
            })
            .catch(() => {
                // Not signed in yet, or the server is down - the app's other queries surface that.
            });
        return () => {
            cancelled = true;
        };
    }, [sync]);

    useEffect(() => {
        const source = new EventSource("/api/events", {
            withCredentials: true,
        });

        source.onmessage = (msg) => {
            let event: ServerEvent;
            try {
                event = JSON.parse(msg.data) as ServerEvent;
            } catch {
                return;
            }

            if (event.type === "feed_polled") {
                progress(event.done, event.total);
                toast.loading("Ingesting your feeds…", {
                    id: toastId(event.run_id),
                    description: `${event.done} of ${event.total} sources polled`,
                    duration: Number.POSITIVE_INFINITY,
                });
                return;
            }

            if (event.type === "ingest_finished") {
                const { run_id, new_items, failed, timed_out } = event;
                if (timed_out) {
                    void settle(
                        run_id,
                        "Ingestion is taking longer than expected - showing what arrived so far",
                        false,
                    );
                } else if (failed > 0) {
                    const items = itemCount(new_items);
                    void settle(
                        run_id,
                        `Ingest finished with errors - ${items}, ${failed} ${failed === 1 ? "feed" : "feeds"} failed`,
                        false,
                    );
                } else {
                    void settle(
                        run_id,
                        `Ingest complete - ${itemCount(new_items)}`,
                        true,
                    );
                }
            }
        };

        // EventSource reconnects on its own. What it can't do is tell us what we missed while it
        // was down, so resync once we're back.
        source.onerror = () => {
            if (source.readyState === EventSource.CLOSED) return;
            void refreshFeedData(qc);
        };

        return () => source.close();
    }, [qc, progress, settle]);

    // Belt and braces: if the run's completion never arrives, refetch anyway and let the toast go.
    // A spinner that can hang forever is worse than one that gives up honestly.
    const timer = useRef<number | null>(null);
    useEffect(() => {
        if (runId === null) return;
        timer.current = window.setTimeout(() => {
            void settle(
                runId,
                "Lost track of the ingestion - refreshed your feed anyway",
                false,
            );
        }, RUN_TIMEOUT_MS);
        return () => {
            if (timer.current !== null) window.clearTimeout(timer.current);
        };
    }, [runId, settle]);
}

function itemCount(n: number): string {
    if (n === 0) return "no new items";
    return `${n} new ${n === 1 ? "item" : "items"}`;
}
