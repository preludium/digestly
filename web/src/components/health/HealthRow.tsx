import { Badge } from "@/components/ui/badge";
import { TableCell, TableRow } from "@/components/ui/table";
import type { FeedHealth } from "@/lib/types";
import { ActionsMenu } from "./ActionsMenu";
import { FeedIcon } from "./FeedIcon";
import { LastFetchToggle } from "./LastFetchToggle";
import { STATUS_BADGE } from "./statusBadge";

export function HealthRow({
    row,
    onEdit,
}: {
    row: FeedHealth;
    onEdit: (id: number) => void;
}) {
    const badge = STATUS_BADGE[row.status];

    return (
        <TableRow>
            <TableCell>
                <div className="flex items-center gap-2">
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
            </TableCell>
            <TableCell>
                <Badge variant={badge.variant}>{badge.label}</Badge>
            </TableCell>
            <TableCell className="text-muted-foreground">
                <LastFetchToggle row={row} />
            </TableCell>
            <TableCell className="text-muted-foreground">
                {row.failure_count}
            </TableCell>
            <TableCell>
                <div className="flex justify-end">
                    <ActionsMenu row={row} onEdit={onEdit} />
                </div>
            </TableCell>
        </TableRow>
    );
}
