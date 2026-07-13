import {
    AlertTriangle,
    Bell,
    BellOff,
    Calendar,
    Newspaper,
} from "lucide-react";
import { Link } from "react-router-dom";
import { EmptyState } from "@/components/common/EmptyState";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { Card } from "@/components/ui/card";
import { Spinner } from "@/components/ui/spinner";
import { useDigests } from "@/hooks/useDigest";
import {
    formatDayHeading,
    formatShortDate,
    formatTimeOfDay,
} from "@/lib/format";

/** Digests list (prompt.md §9.8): the current user's archived digests, newest first. */
export function Digests() {
    const digests = useDigests();

    return (
        <div className="space-y-4">
            <h1 className="font-display text-2xl font-semibold tracking-tight">
                Digests
            </h1>

            {digests.isLoading ? (
                <div className="flex justify-center py-10">
                    <Spinner className="size-6" />
                </div>
            ) : digests.isError ? (
                <ErrorBanner error={digests.error} />
            ) : digests.data && digests.data.length > 0 ? (
                <ul className="space-y-2">
                    {digests.data.map((d) => (
                        <li key={d.id}>
                            <Link to={`/digests/${d.id}`}>
                                <Card className="flex items-center gap-4 p-4 transition-colors hover:bg-muted/50">
                                    <div className="min-w-0 flex-1">
                                        <div className="flex flex-wrap items-center gap-2.5">
                                            <p className="font-display text-[17px] font-semibold tracking-tight">
                                                {formatDayHeading(d.created_at)}
                                            </p>
                                            <span className="inline-flex items-center whitespace-nowrap rounded-md bg-muted px-2 py-0.5 text-[11px] font-semibold text-muted-foreground">
                                                {formatTimeOfDay(d.created_at)}
                                            </span>
                                        </div>
                                        <p className="mt-1 flex items-center gap-1.5 text-xs text-muted-foreground">
                                            <Calendar className="size-3 shrink-0" />
                                            {formatShortDate(d.period_start)} –{" "}
                                            {formatShortDate(d.period_end)}
                                        </p>
                                    </div>

                                    {d.error && (
                                        <span className="inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap rounded-full bg-warning/15 px-3 py-1 text-xs font-semibold text-warning">
                                            <AlertTriangle className="size-3.5" />{" "}
                                            Delivery failed
                                        </span>
                                    )}

                                    <span className="inline-flex shrink-0 items-baseline gap-1">
                                        <span className="text-lg font-bold">
                                            {d.item_count}
                                        </span>
                                        <span className="text-xs text-muted-foreground">
                                            items
                                        </span>
                                    </span>

                                    <div className="flex shrink-0 items-center gap-2 self-stretch border-l border-border pl-4 text-muted-foreground">
                                        {d.notified ? (
                                            <Bell
                                                className="size-4 text-primary"
                                                aria-label="Pushed to ntfy"
                                            />
                                        ) : (
                                            <BellOff
                                                className="size-4"
                                                aria-label="Not pushed"
                                            />
                                        )}
                                    </div>
                                </Card>
                            </Link>
                        </li>
                    ))}
                </ul>
            ) : (
                <EmptyState
                    icon={<Newspaper className="size-8" />}
                    title="No digests yet"
                    description="Digests appear here on the schedule set by your admin, or after a manual run."
                />
            )}
        </div>
    );
}
