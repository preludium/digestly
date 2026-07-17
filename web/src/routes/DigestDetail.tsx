import { AlertTriangle, ArrowLeft, Calendar, Sparkles } from "lucide-react";
import { Link, useParams } from "react-router-dom";
import { EmptyState } from "@/components/common/EmptyState";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { Markdown } from "@/components/common/Markdown";
import { PageTitle } from "@/components/common/PageHeadings";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Spinner } from "@/components/ui/spinner";
import { useDigest } from "@/hooks/useDigest";
import {
    formatDayHeading,
    formatShortDate,
    formatTimeOfDay,
} from "@/lib/format";
import type { DigestCategorySection } from "@/lib/types";

/** Digest detail (prompt.md §9.9): per-category sections (name → AI summary → source links),
 *  fetch-failure warning, and a raw-fallback note when AI was unavailable. */
export function DigestDetail() {
    const { id } = useParams();
    const digest = useDigest(Number(id));

    return (
        <div className="space-y-4">
            <Link
                to="/digests"
                className="inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground"
            >
                <ArrowLeft className="size-4" /> Back to digests
            </Link>

            {digest.isLoading ? (
                <div className="flex justify-center py-10">
                    <Spinner className="size-6" />
                </div>
            ) : digest.isError ? (
                <ErrorBanner error={digest.error} />
            ) : digest.data ? (
                <Body
                    payload={digest.data.payload}
                    createdAt={digest.data.created_at}
                    itemCount={digest.data.item_count}
                />
            ) : null}
        </div>
    );
}

function Body({
    payload,
    createdAt,
    itemCount,
}: {
    payload: import("@/lib/types").DigestPayload | null;
    createdAt: string;
    itemCount: number;
}) {
    if (!payload) {
        return (
            <EmptyState
                title="Digest unavailable"
                description="This digest has no stored content."
            />
        );
    }

    return (
        <div className="space-y-4">
            <header className="flex flex-wrap items-center gap-x-3.5 gap-y-2">
                <PageTitle>{formatDayHeading(createdAt)}</PageTitle>
                <span className="inline-flex items-center whitespace-nowrap rounded-md bg-muted px-2.5 py-0.5 text-xs font-semibold text-muted-foreground">
                    {formatTimeOfDay(createdAt)}
                </span>
                <span className="w-full" />
                <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
                    <Calendar className="size-3 shrink-0" />
                    {formatShortDate(payload.period_start)} –{" "}
                    {formatShortDate(payload.period_end)}
                </p>
                <span className="inline-flex items-baseline gap-1">
                    <span className="text-sm font-bold">{itemCount}</span>
                    <span className="text-xs text-muted-foreground">
                        item{itemCount === 1 ? "" : "s"}
                    </span>
                </span>
            </header>

            {payload.failure_warning && (
                <Alert variant="destructive">
                    <AlertTriangle className="size-4" />
                    {payload.failure_warning}
                </Alert>
            )}
            {payload.fallback_note && <Alert>{payload.fallback_note}</Alert>}

            {payload.categories.length === 0 ? (
                <EmptyState
                    title="Nothing new"
                    description="No items were published in this digest's window."
                />
            ) : (
                payload.categories.map((section) => (
                    <Section key={section.name} section={section} />
                ))
            )}
        </div>
    );
}

function Section({ section }: { section: DigestCategorySection }) {
    const groups = groupByFeed(section.items);
    return (
        <Card>
            <CardHeader className="flex-row items-center justify-between gap-2 pb-3">
                <CardTitle className="text-base">{section.name}</CardTitle>
                <div className="flex items-center gap-2">
                    <Badge>{section.items.length}</Badge>
                    {!section.raw && (
                        <span className="inline-flex items-center gap-1 text-xs text-muted-foreground">
                            <Sparkles className="size-3" /> AI
                        </span>
                    )}
                </div>
            </CardHeader>
            <CardContent className="space-y-3.5">
                {section.ai_summary ? (
                    <Markdown className="text-sm">
                        {section.ai_summary}
                    </Markdown>
                ) : (
                    <p className="text-sm text-muted-foreground">
                        Raw headlines (no AI summary):
                    </p>
                )}
                <div className="space-y-3.5 pt-0.5">
                    {groups.map((g) => (
                        <div key={g.feed}>
                            <div className="mb-1.5 flex items-center gap-2">
                                <span className="text-xs font-bold tracking-wide">
                                    {g.feed}
                                </span>
                                <span className="text-[11px] font-semibold text-muted-foreground">
                                    {g.items.length}
                                </span>
                                <span className="h-px flex-1 bg-border" />
                            </div>
                            <ul className="space-y-1 pl-0.5 text-sm">
                                {g.items.map((it, i) => (
                                    // biome-ignore lint/suspicious/noArrayIndexKey: existing baseline
                                    <li key={i}>
                                        {it.url ? (
                                            <a
                                                href={it.url}
                                                target="_blank"
                                                rel="noreferrer"
                                                className="text-primary hover:underline"
                                            >
                                                {it.title}
                                            </a>
                                        ) : (
                                            <span>{it.title}</span>
                                        )}
                                    </li>
                                ))}
                            </ul>
                        </div>
                    ))}
                </div>
            </CardContent>
        </Card>
    );
}

/** Group a section's flat item list by feed, preserving first-seen order (mockup: sections show
 *  a feed-name/count divider, then just the titles - not "title - feed" repeated per row). */
function groupByFeed(items: DigestCategorySection["items"]) {
    const byFeed = new Map<string, DigestCategorySection["items"]>();
    for (const it of items) {
        const list = byFeed.get(it.feed_title) ?? [];
        list.push(it);
        byFeed.set(it.feed_title, list);
    }
    return Array.from(byFeed.entries()).map(([feed, feedItems]) => ({
        feed,
        items: feedItems,
    }));
}
