import {
    HeartPulse,
    MoreVertical,
    Pencil,
    RefreshCw,
    Search as SearchIcon,
    Trash2,
} from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { EmptyState } from "@/components/common/EmptyState";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { FeedEditModal } from "@/components/feeds/FeedEditModal";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import {
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
    Table,
    TableBody,
    TableCell,
    TableHead,
    TableHeader,
    TableRow,
} from "@/components/ui/table";
import {
    useFeedHealth,
    useFeeds,
    useRefreshFeed,
    useUnsubscribe,
} from "@/hooks/useFeeds";
import { formatDateTime, kindIcon, relativeTime } from "@/lib/format";
import type { Feed, FeedHealth, FeedStatus } from "@/lib/types";

const STATUS_BADGE: Record<
    FeedStatus,
    { label: string; variant: "success" | "warning" | "destructive" }
> = {
    ok: { label: "Ok", variant: "success" },
    failing: { label: "Failing", variant: "warning" },
    disabled: { label: "Disabled", variant: "destructive" },
};

function YoutubeIcon({ className }: { className?: string }) {
    return (
        <svg
            className={className}
            viewBox="0 0 24 24"
            fill="currentColor"
            aria-label="YouTube"
        >
            <path d="M19.615 3.184c-3.604-.246-11.631-.245-15.23 0-3.897.266-4.356 2.62-4.385 8.816.029 6.185.484 8.549 4.385 8.816 3.6.245 11.626.246 15.23 0 3.897-.266 4.356-2.62 4.385-8.816-.029-6.185-.484-8.549-4.385-8.816zm-10.615 12.816v-8l8 3.993-8 4.007z" />
        </svg>
    );
}

/** Feed health / diagnostics (prompt.md §9.6). Failing/disabled feeds surfaced, never dropped. */
export function Health() {
    const health = useFeedHealth();
    const feeds = useFeeds();
    const [search, setSearch] = useState("");
    const [statusFilter, setStatusFilter] = useState<"all" | FeedStatus>("all");
    const [editing, setEditing] = useState<Feed | null>(null);

    const query = search.trim().toLowerCase();
    const rows = (health.data ?? []).filter(
        (h) =>
            (statusFilter === "all" || h.status === statusFilter) &&
            (!query || h.title.toLowerCase().includes(query)),
    );
    const noFeeds = (health.data?.length ?? 0) === 0;
    const filtered = statusFilter !== "all" || query.length > 0;

    const openEdit = (id: number) => {
        const feed = feeds.data?.find((f) => f.id === id);
        if (feed) setEditing(feed);
    };

    return (
        <div className="space-y-6">
            <div className="flex flex-wrap items-center justify-between gap-3">
                <h1 className="font-display text-2xl font-semibold tracking-tight">
                    Feed health
                </h1>
                <div className="flex flex-wrap items-center gap-3.5">
                    <div className="relative">
                        <SearchIcon className="pointer-events-none absolute left-2.5 top-1/2 size-[15px] -translate-y-1/2 text-muted-foreground" />
                        <Input
                            aria-label="Search feeds"
                            placeholder="Search feeds"
                            value={search}
                            onChange={(e) => setSearch(e.target.value)}
                            className="h-[34px] w-[170px] pl-8 text-[13px]"
                        />
                    </div>
                    <label className="flex items-center gap-1.5 text-sm">
                        <span className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
                            Status
                        </span>
                        <Select
                            value={statusFilter}
                            onValueChange={(v) =>
                                setStatusFilter(v as "all" | FeedStatus)
                            }
                        >
                            <SelectTrigger className="h-[34px] w-auto gap-1.5 text-[13px] font-semibold">
                                <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                                <SelectItem value="all">All</SelectItem>
                                <SelectItem value="ok">Ok</SelectItem>
                                <SelectItem value="failing">Failing</SelectItem>
                                <SelectItem value="disabled">
                                    Disabled
                                </SelectItem>
                            </SelectContent>
                        </Select>
                    </label>
                </div>
            </div>

            {health.isError && <ErrorBanner error={health.error} />}

            {health.isLoading && (
                <div className="space-y-2">
                    {[0, 1, 2].map((i) => (
                        <Skeleton key={i} className="h-12 w-full" />
                    ))}
                </div>
            )}

            {!health.isLoading &&
                !health.isError &&
                (noFeeds || rows.length === 0) && (
                    <EmptyState
                        icon={<HeartPulse className="size-8" />}
                        title={
                            noFeeds
                                ? "No feeds yet"
                                : filtered
                                  ? "No feeds match"
                                  : "All feeds healthy"
                        }
                        description={
                            noFeeds
                                ? "Add a feed to see its health here."
                                : filtered
                                  ? "Try a different search or status filter."
                                  : "Nothing to worry about right now 🎉"
                        }
                    />
                )}

            {!health.isLoading && !health.isError && rows.length > 0 && (
                <>
                    {/* Mobile: stacked cards - avoids a wide table forcing horizontal scroll to reach
                     *  the actions menu (the trigger would sit far outside the visible viewport). */}
                    <ul className="space-y-2 sm:hidden">
                        {rows.map((h) => (
                            <HealthCard key={h.id} row={h} onEdit={openEdit} />
                        ))}
                    </ul>

                    {/* Desktop / tablet: full table. */}
                    <Card className="hidden sm:block">
                        <CardContent className="overflow-x-auto">
                            <Table>
                                <TableHeader>
                                    <TableRow className="hover:bg-card">
                                        <TableHead>Feed</TableHead>
                                        <TableHead>Status</TableHead>
                                        <TableHead>Last fetch</TableHead>
                                        <TableHead>Fails</TableHead>
                                        <TableHead className="text-right">
                                            <span className="sr-only">
                                                Actions
                                            </span>
                                        </TableHead>
                                    </TableRow>
                                </TableHeader>
                                <TableBody>
                                    {rows.map((h) => (
                                        <HealthRow
                                            key={h.id}
                                            row={h}
                                            onEdit={openEdit}
                                        />
                                    ))}
                                </TableBody>
                            </Table>
                        </CardContent>
                    </Card>
                </>
            )}

            <FeedEditModal feed={editing} onClose={() => setEditing(null)} />
        </div>
    );
}

function FeedIcon({ row }: { row: FeedHealth }) {
    return row.kind === "youtube" ? (
        <YoutubeIcon className="size-5 shrink-0 text-destructive" />
    ) : (
        <span className="text-lg">{kindIcon(row.kind)}</span>
    );
}

function LastFetchToggle({ row }: { row: FeedHealth }) {
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

function ActionsMenu({
    row,
    onEdit,
}: {
    row: FeedHealth;
    onEdit: (id: number) => void;
}) {
    const refresh = useRefreshFeed();
    const unsubscribe = useUnsubscribe();
    const [removing, setRemoving] = useState<FeedHealth | null>(null);

    return (
        <>
            <DropdownMenu>
                <DropdownMenuTrigger asChild>
                    <Button variant="ghost" size="icon" aria-label="Actions">
                        <MoreVertical className="size-4" />
                    </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                    <DropdownMenuItem
                        disabled={refresh.isPending}
                        onClick={() =>
                            refresh.mutate(row.id, {
                                onSuccess: () =>
                                    toast.success(
                                        row.status === "disabled"
                                            ? "Re-enabled"
                                            : "Retrying…",
                                    ),
                            })
                        }
                    >
                        <RefreshCw className="size-4" />{" "}
                        {row.status === "disabled" ? "Re-enable" : "Retry"}
                    </DropdownMenuItem>
                    <DropdownMenuItem onClick={() => onEdit(row.id)}>
                        <Pencil className="size-4" /> Edit
                    </DropdownMenuItem>
                    <DropdownMenuItem
                        className="text-destructive focus:bg-destructive/10 focus:text-destructive"
                        onClick={() => setRemoving(row)}
                    >
                        <Trash2 className="size-4" /> Remove
                    </DropdownMenuItem>
                </DropdownMenuContent>
            </DropdownMenu>
            <ConfirmDialog
                open={!!removing}
                onOpenChange={(v) => !v && setRemoving(null)}
                title={`Unsubscribe from "${row.title}"?`}
                confirmLabel="Unsubscribe"
                destructive
                onConfirm={() => {
                    unsubscribe.mutate(removing!.id, {
                        onSuccess: () => {
                            toast.success("Unsubscribed");
                            setRemoving(null);
                        },
                    });
                }}
            />
        </>
    );
}

function HealthCard({
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

function HealthRow({
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
