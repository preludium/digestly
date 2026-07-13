import {
    Check,
    ChevronDown,
    ExternalLink,
    MessageSquare,
    Sparkles,
    Star,
    TrendingUp,
} from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { Markdown } from "@/components/common/Markdown";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Sheet, SheetContent } from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import {
    useItem,
    useSummarize,
    useToggleRead,
    useToggleStar,
} from "@/hooks/useItems";
import {
    formatDuration,
    readingTimeLabel,
    relativeTimeLong,
} from "@/lib/format";
import { topicBadgeClass } from "@/lib/topicColor";
import type { Item, ItemDetail } from "@/lib/types";
import { cn } from "@/lib/utils";

/** The reading surface (§9.1a). Full-width overlay on mobile, right sheet ≥820px. Reading items
 *  render sanitized HTML; video items are shown AS TEXT (summary slot → transcript → watch link).
 *  Opening an item marks it read. */
export function ItemPreview({
    item,
    onClose,
}: {
    item: Item | null;
    onClose: () => void;
}) {
    const detailQuery = useItem(item?.id ?? null);
    const detail = detailQuery.data;
    const toggleStar = useToggleStar();
    const toggleRead = useToggleRead();

    // The card gives us instant header data; detail fills in body + fresh state.
    const view = (detail ?? item) as (Item & Partial<ItemDetail>) | null;

    return (
        <Sheet open={item != null} onOpenChange={(o) => !o && onClose()}>
            <SheetContent
                side="right"
                showClose={false}
                className="flex w-full flex-col gap-0 overflow-y-auto p-0 sm:max-w-xl"
            >
                {view && (
                    <>
                        {/* Action bar */}
                        <div className="sticky top-0 z-10 flex items-center gap-2 border-b border-border bg-card px-4 py-3">
                            <Button variant="ghost" size="sm" onClick={onClose}>
                                ← Back
                            </Button>
                            <div className="ml-auto flex items-center gap-1">
                                <Button
                                    variant="ghost"
                                    size="icon"
                                    aria-label={
                                        view.is_starred ? "Unstar" : "Star"
                                    }
                                    onClick={() =>
                                        toggleStar.mutate({
                                            id: view.id,
                                            value: !view.is_starred,
                                        })
                                    }
                                >
                                    <Star
                                        className={cn(
                                            "size-5",
                                            view.is_starred &&
                                                "fill-star text-star",
                                        )}
                                    />
                                </Button>
                                <Button
                                    variant={
                                        view.is_read ? "secondary" : "default"
                                    }
                                    size="sm"
                                    onClick={() =>
                                        toggleRead.mutate({
                                            id: view.id,
                                            value: !view.is_read,
                                        })
                                    }
                                >
                                    {view.is_read ? (
                                        <>
                                            <Check className="size-4" /> Mark as
                                            unread
                                        </>
                                    ) : (
                                        "Mark as read"
                                    )}
                                </Button>
                            </div>
                        </div>

                        <div className="flex-1 px-4 py-4">
                            {detailQuery.isError ? (
                                <ErrorBanner error={detailQuery.error} />
                            ) : (
                                <PreviewBody
                                    view={view}
                                    loading={detailQuery.isLoading}
                                    onOpen={() =>
                                        markReadOnOpen(view, toggleRead)
                                    }
                                />
                            )}
                        </div>
                    </>
                )}
            </SheetContent>
        </Sheet>
    );
}

/** Mark an item read the first time its preview loads (auto-mark-on-open). */
function markReadOnOpen(
    view: Item,
    toggleRead: ReturnType<typeof useToggleRead>,
) {
    if (!view.is_read) toggleRead.mutate({ id: view.id, value: true });
}

function PreviewBody({
    view,
    loading,
    onOpen,
}: {
    view: Item & Partial<ItemDetail>;
    loading: boolean;
    onOpen: () => void;
}) {
    const isVideo = view.content_type === "video";
    const timeLabel = readingTimeLabel(
        view.reading_time_secs,
        view.content_type,
    );

    return (
        <article className="space-y-4">
            <div className="flex flex-wrap items-center gap-1.5">
                <Badge variant="secondary">
                    {isVideo ? "🎬 Video" : "📖 Reading"}
                </Badge>
                <Badge
                    variant="secondary"
                    className={cn(topicBadgeClass(view.category))}
                >
                    {view.category}
                </Badge>
            </div>

            <h1 className="text-xl font-bold leading-tight">
                {view.title ?? "Untitled"}
            </h1>

            <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 text-xs text-muted-foreground">
                <span className="font-medium text-foreground">
                    {view.feed_title}
                </span>
                {view.published_at && (
                    <span>{relativeTimeLong(view.published_at)}</span>
                )}
                <div className="flex flex-wrap items-center gap-1.5">
                    {timeLabel && (
                        <Badge variant="secondary">{timeLabel}</Badge>
                    )}
                    {isVideo && formatDuration(view.duration_secs) && (
                        <Badge variant="secondary">
                            ⏱ {formatDuration(view.duration_secs)}
                        </Badge>
                    )}
                    {view.score != null && (
                        <Badge variant="secondary" className="gap-1">
                            <TrendingUp className="size-3" /> {view.score}
                        </Badge>
                    )}
                    {view.comments_count != null && (
                        <Badge variant="secondary" className="gap-1">
                            <MessageSquare className="size-3" />{" "}
                            {view.comments_count}
                        </Badge>
                    )}
                </div>
            </div>

            {view.url && (
                <div className="flex flex-wrap gap-2">
                    <Button variant="default" size="sm" asChild>
                        <a
                            href={view.url}
                            target="_blank"
                            rel="noreferrer"
                            onClick={onOpen}
                        >
                            <ExternalLink className="size-4" /> Open original
                        </a>
                    </Button>
                </div>
            )}

            {loading ? (
                <div className="space-y-2 pt-2">
                    <Skeleton className="h-4 w-full" />
                    <Skeleton className="h-4 w-5/6" />
                    <Skeleton className="h-4 w-2/3" />
                </div>
            ) : isVideo ? (
                <VideoBody view={view} />
            ) : (
                <ReadingBody view={view} />
            )}
        </article>
    );
}

function ReadingBody({ view }: { view: Item & Partial<ItemDetail> }) {
    if (view.content_html) {
        return (
            <div
                className="article-content"
                dangerouslySetInnerHTML={{ __html: view.content_html }}
            />
        );
    }
    return <SummarySlot view={view} />;
}

function VideoBody({ view }: { view: Item & Partial<ItemDetail> }) {
    const [showTranscript, setShowTranscript] = useState(false);
    const unavailable = view.transcript_status === "unavailable";

    return (
        <div className="space-y-4">
            {/* 1. AI summary (primary) - filled in Phase 5 */}
            <SummarySlot view={view} />

            {/* 2. Collapsible transcript */}
            <div className="rounded-md border border-border">
                <button
                    type="button"
                    className="flex w-full items-center justify-between px-3 py-2 text-sm font-medium"
                    onClick={() => setShowTranscript((s) => !s)}
                >
                    📄{" "}
                    {unavailable
                        ? "No captions available"
                        : "Show full transcript"}
                    <ChevronDown
                        className={cn(
                            "size-4 transition-transform",
                            showTranscript && "rotate-180",
                        )}
                    />
                </button>
                {showTranscript && (
                    <div className="border-t border-border px-3 py-2 text-sm text-muted-foreground">
                        {view.transcript_text ? (
                            <p className="whitespace-pre-wrap leading-relaxed">
                                {view.transcript_text}
                            </p>
                        ) : unavailable ? (
                            <p>
                                No captions were available for this video; a
                                description-based summary is shown above.
                            </p>
                        ) : (
                            <p>The transcript hasn’t been fetched yet.</p>
                        )}
                    </div>
                )}
            </div>
        </div>
    );
}

/** AI summary slot. Renders the cached summary (with a Regenerate action) or a Summarize
 *  affordance; both call the real on-demand endpoint (§6, §6a). Errors (no provider, budget,
 *  provider failure) surface as a toast - never a crash or dead button (§11). */
function SummarySlot({ view }: { view: Item & Partial<ItemDetail> }) {
    const summarize = useSummarize();

    const run = (force: boolean) =>
        summarize.mutate(
            { id: view.id, force },
            {
                onError: (e) =>
                    toast.error(
                        e instanceof Error ? e.message : "Could not summarize",
                    ),
            },
        );

    if (view.summary) {
        return (
            <div className="rounded-md border border-border bg-muted/40 p-3">
                <div className="mb-1 flex items-center justify-between gap-2">
                    <p className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                        AI summary
                    </p>
                    <Button
                        variant="ghost"
                        size="sm"
                        disabled={summarize.isPending}
                        onClick={() => run(true)}
                    >
                        <Sparkles
                            className={cn(
                                "size-4",
                                summarize.isPending && "animate-pulse",
                            )}
                        />{" "}
                        Regenerate
                    </Button>
                </div>
                <Markdown>{view.summary}</Markdown>
            </div>
        );
    }

    return (
        <div className="flex flex-col items-start gap-2 rounded-md border border-dashed border-border p-3">
            <p className="text-sm text-muted-foreground">
                {view.content_type === "video"
                    ? "Read this video as text - generate an AI summary of its transcript."
                    : "Generate an AI summary of this article."}
            </p>
            <Button
                variant="outline"
                size="sm"
                disabled={summarize.isPending}
                onClick={() => run(false)}
            >
                <Sparkles
                    className={cn(
                        "size-4",
                        summarize.isPending && "animate-pulse",
                    )}
                />
                {summarize.isPending ? "Summarizing…" : "Summarize"}
            </Button>
        </div>
    );
}
