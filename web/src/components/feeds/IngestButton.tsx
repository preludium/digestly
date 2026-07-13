import { RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { useIngestNow } from "@/hooks/useIngest";
import { cn } from "@/lib/utils";
import { useIngestStore } from "@/stores/ingest";

/** "Ingest now" (§9.0) - poll every subscribed feed right now.
 *
 *  Its state comes from the run, not from the mutation: the request returns in milliseconds
 *  (it only marks feeds due), so `isPending` would flash and clear long before any feed is
 *  actually polled. The run is what the user means by "ingesting", and it survives navigation. */
export function IngestButton() {
    const ingest = useIngestNow();
    const running = useIngestStore((s) => s.runId !== null);
    const cooldown = useCooldown();
    const busy = running || ingest.isPending;

    return (
        <Button
            onClick={() => ingest.mutate()}
            disabled={busy || cooldown > 0}
            aria-label={
                busy
                    ? "Ingesting feeds"
                    : cooldown > 0
                      ? `Ingest now - available in ${cooldown} seconds`
                      : "Ingest now"
            }
        >
            <RefreshCw className={cn("size-4.5", busy && "animate-spin")} />
            {busy ? "Ingesting…" : "Ingest now"}
            {!busy && cooldown > 0 && (
                <span className="tabular-nums opacity-60">{cooldown}s</span>
            )}
        </Button>
    );
}

/** Seconds left on the ingest cooldown, ticking down to 0. Mirrors the server's floor on manual
 *  ingestion - hammering "Ingest now" is what gets an instance soft-blocked by YouTube/Reddit. */
function useCooldown(): number {
    const until = useIngestStore((s) => s.cooldownUntil);
    const [left, setLeft] = useState(() => remaining(until));

    useEffect(() => {
        setLeft(remaining(until));
        if (until <= Date.now()) return;
        const id = window.setInterval(() => {
            const secs = remaining(until);
            setLeft(secs);
            if (secs === 0) window.clearInterval(id);
        }, 500);
        return () => window.clearInterval(id);
    }, [until]);

    return left;
}

function remaining(until: number): number {
    return Math.max(0, Math.ceil((until - Date.now()) / 1000));
}
