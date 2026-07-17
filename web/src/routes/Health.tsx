import { HeartPulse, Search as SearchIcon } from "lucide-react";
import { useState } from "react";
import { EmptyState } from "@/components/common/EmptyState";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { PageTitle } from "@/components/common/PageHeadings";
import { FeedEditModal } from "@/components/feeds/FeedEditModal";
import { HealthCard } from "@/components/health/HealthCard";
import { HealthRow } from "@/components/health/HealthRow";
import { Card, CardContent } from "@/components/ui/card";
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
    TableHead,
    TableHeader,
    TableRow,
} from "@/components/ui/table";
import { useFeedHealth, useFeeds } from "@/hooks/useFeeds";
import type { Feed, FeedStatus } from "@/lib/types";

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
                <PageTitle>Feed health</PageTitle>
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
                    {/* biome-ignore lint/a11y/noLabelWithoutControl: existing baseline */}
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
