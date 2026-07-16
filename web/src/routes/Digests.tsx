import {
    AlertTriangle,
    Bell,
    BellOff,
    Calendar,
    Newspaper,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import { EmptyState } from "@/components/common/EmptyState";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { Card } from "@/components/ui/card";
import { Spinner } from "@/components/ui/spinner";
import { useMe } from "@/hooks/useAuth";
import { useDigestSchedule, useDigests } from "@/hooks/useDigest";
import {
    formatDateTime,
    formatDayHeading,
    formatShortDate,
    formatTimeOfDay,
} from "@/lib/format";

/** Digests list (prompt.md §9.8): the current user's archived digests, newest first. */
export function Digests() {
    const digests = useDigests();
    const schedule = useDigestSchedule();
    const { data: me } = useMe();
    const [now, setNow] = useState(() => Date.now());
    const expiredSchedule = useRef<string | null>(null);
    const nextRunAt = schedule.data?.next_run_at ?? null;
    const countdown = nextRunAt ? formatCountdown(nextRunAt, now) : null;
    const { refetch } = schedule;

    useEffect(() => {
        const id = window.setInterval(() => setNow(Date.now()), 60_000);
        return () => window.clearInterval(id);
    }, []);

    useEffect(() => {
        if (
            schedule.data?.enabled &&
            nextRunAt &&
            !countdown &&
            expiredSchedule.current !== nextRunAt
        ) {
            expiredSchedule.current = nextRunAt;
            void refetch();
        } else if (countdown) {
            expiredSchedule.current = null;
        }
    }, [countdown, nextRunAt, refetch, schedule.data?.enabled]);

    return (
        <div className="space-y-4">
            <h1 className="font-display text-2xl font-semibold tracking-tight">
                Digests
            </h1>

            {schedule.data && (
                <Card className="flex flex-wrap items-center gap-3 p-4">
                    <Calendar className="size-5 shrink-0 text-primary" />
                    <div className="min-w-0 flex-1">
                        {schedule.data.enabled ? (
                            <p className="font-semibold">
                                {countdown
                                    ? `Next digest in ${countdown}`
                                    : "Updating next digest time…"}
                            </p>
                        ) : (
                            <p className="font-semibold">
                                Scheduled digests are paused
                            </p>
                        )}
                        <p className="text-sm text-muted-foreground">
                            {schedule.data.description} ·{" "}
                            {schedule.data.timezone}
                            {schedule.data.enabled && nextRunAt
                                ? ` · ${formatDateTime(nextRunAt)}`
                                : ""}
                        </p>
                    </div>
                    {me?.role === "admin" && (
                        <Link
                            to="/admin/system"
                            className="text-sm font-semibold text-primary hover:underline"
                        >
                            Digest settings
                        </Link>
                    )}
                </Card>
            )}

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
                    description={
                        schedule.data
                            ? `Digests appear here ${schedule.data.description}.`
                            : "Digests appear here after a scheduled or manual run."
                    }
                />
            )}
        </div>
    );
}

function formatCountdown(nextRunAt: string, now: number): string | null {
    const nextRun = new Date(nextRunAt).getTime();
    if (Number.isNaN(nextRun)) return null;

    const minutes = Math.ceil((nextRun - now) / 60_000);
    if (minutes <= 0) return null;

    const hours = Math.floor(minutes / 60);
    return hours > 0 ? `${hours}h ${minutes % 60}m` : `${minutes}m`;
}
