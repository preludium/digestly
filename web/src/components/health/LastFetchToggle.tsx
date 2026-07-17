import { useState } from "react";
import { formatDateTime, relativeTime } from "@/lib/format";
import type { FeedHealth } from "@/lib/types";

export function LastFetchToggle({ row }: { row: FeedHealth }) {
    const [showFull, setShowFull] = useState(false);
    return (
        <button
            type="button"
            onClick={() => setShowFull((v) => !v)}
            title={formatDateTime(row.last_fetch_at)}
            className="whitespace-nowrap underline decoration-border decoration-dotted underline-offset-4"
        >
            {showFull
                ? formatDateTime(row.last_fetch_at)
                : relativeTime(row.last_fetch_at) || "Never"}
        </button>
    );
}
