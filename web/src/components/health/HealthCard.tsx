import { Badge } from "@/components/ui/badge";
import type { FeedHealth } from "@/lib/types";
import { ActionsMenu } from "./ActionsMenu";
import { FeedIcon } from "./FeedIcon";
import { LastFetchToggle } from "./LastFetchToggle";
import { STATUS_BADGE } from "./statusBadge";

export function HealthCard({
    row,
    onEdit,
}: {
    row: FeedHealth;
    onEdit: (id: number) => void;
}) {
    const badge = STATUS_BADGE[row.status];

    return (
        <li className="rounded-lg border border-border bg-card p-3 shadow-sm">
            <div className="flex items-start justify-between gap-2">
                <div className="flex min-w-0 items-center gap-2">
                    <FeedIcon row={row} />
                    <div className="min-w-0">
                        <p className="truncate font-medium">{row.title}</p>
                        {row.last_error && (
                            <p className="truncate text-xs text-destructive">
                                {row.last_error}
                            </p>
                        )}
                    </div>
                </div>
                <ActionsMenu row={row} onEdit={onEdit} />
            </div>
            <div className="mt-2 flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
                <Badge variant={badge.variant}>{badge.label}</Badge>
                <span>
                    Last fetch: <LastFetchToggle row={row} />
                </span>
                <span>Fails: {row.failure_count}</span>
            </div>
        </li>
    );
}
