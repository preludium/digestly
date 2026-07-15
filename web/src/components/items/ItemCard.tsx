import { ImageIcon, MessageSquare, Play, Star, TrendingUp } from "lucide-react";
import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import {
    formatDuration,
    readingTimeLabel,
    relativeTimeLong,
} from "@/lib/format";
import { highlight } from "@/lib/highlight";
import { topicBadgeClass } from "@/lib/topicColor";
import type { Item } from "@/lib/types";
import { cn } from "@/lib/utils";

function RedditLogo() {
    return (
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 24 24"
            fill="currentColor"
            className="size-8"
            aria-hidden="true"
        >
            <path d="M12 0A12 12 0 0 0 0 12a12 12 0 0 0 12 12 12 12 0 0 0 12-12A12 12 0 0 0 12 0zm5.01 4.744c.688 0 1.25.561 1.25 1.249a1.25 1.25 0 0 1-2.498.056l-2.597-.547.8-3.747c-1.117-.248-2.283-.371-3.495-.391v7.335c.264-.037.537-.07.814-.085a4.07 4.07 0 0 1 3.955 3.632 4.07 4.07 0 0 1-3.94 4.463 4.07 4.07 0 0 1-4.103-3.922 4.068 4.068 0 0 1 2.835-3.861V8.08h.004c-1.947.073-3.786.56-5.192 1.317a1.252 1.252 0 0 1-1.532-1.797c1.623-1.27 3.96-2.09 6.723-2.176v-.038a1.25 1.25 0 0 1-.988-1.642zm-5.146 12.63c-.339 0-.614.202-.614.45 0 .25.275.452.614.452.34 0 .615-.202.615-.451 0-.248-.275-.45-.615-.45zm3.726.902c-1.46 1.46-4.02 1.571-5.37.24a.237.237 0 0 1-.004-.335.237.237 0 0 1 .336-.004c1.113 1.097 3.117.976 4.36-.267.519-.52.78-1.19.78-1.91a.238.238 0 0 1 .476 0c0 .902-.345 1.77-.578 2.276zm.566-1.352c-.34 0-.615.202-.615.45 0 .25.275.452.615.452.339 0 .614-.202.614-.451 0-.248-.275-.45-.614-.45z" />
        </svg>
    );
}

function Thumb({ item }: { item: Item }) {
    const [errored, setErrored] = useState(false);

    if (item.image_url && !errored) {
        return (
            <img
                src={item.image_url}
                alt=""
                loading="lazy"
                className="size-full object-cover"
                onError={() => setErrored(true)}
            />
        );
    }

    if (item.kind === "reddit") {
        return (
            <div className="flex size-full items-center justify-center text-muted-foreground">
                <RedditLogo />
            </div>
        );
    }

    return (
        <div className="flex size-full items-center justify-center text-muted-foreground">
            <ImageIcon className="size-8" />
        </div>
    );
}

/** The single card used by both the feed and search grids (prompt.md §9.1 - DRY, one card). */
export function ItemCard({
    item,
    onOpen,
    query = "",
}: {
    item: Item;
    onOpen: (item: Item) => void;
    query?: string;
}) {
    const isVideo = item.content_type === "video";
    const timeLabel = readingTimeLabel(
        item.reading_time_secs,
        item.content_type,
    );
    const duration = formatDuration(item.duration_secs);

    return (
        <button
            type="button"
            onClick={() => onOpen(item)}
            className={cn(
                "group flex flex-col overflow-hidden rounded-lg border border-border bg-card text-left transition-colors hover:border-primary/50 hover:cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                item.is_read && "opacity-60",
            )}
        >
            {/* Thumbnail */}
            <div className="relative aspect-video w-full overflow-hidden bg-muted">
                <Thumb key={item.id} item={item} />
                {isVideo && (
                    <>
                        <span className="absolute inset-0 flex items-center justify-center">
                            <span className="flex size-11 items-center justify-center rounded-full bg-background/80 text-foreground">
                                <Play className="size-5 fill-current" />
                            </span>
                        </span>
                        {duration && (
                            <span className="absolute bottom-1.5 right-1.5 rounded bg-background/85 px-1.5 py-0.5 text-xs font-medium">
                                {duration}
                            </span>
                        )}
                    </>
                )}
                {item.is_starred && (
                    <span className="absolute right-1.5 top-1.5 flex size-6 items-center justify-center rounded-full bg-background/85">
                        <Star className="size-3.5 fill-star text-star" />
                    </span>
                )}
            </div>

            {/* Body */}
            <div className="flex flex-1 flex-col gap-2 p-3">
                <h3
                    className={cn(
                        "line-clamp-2 font-semibold leading-snug",
                        item.is_read && "text-muted-foreground",
                    )}
                >
                    {highlight(item.title ?? "Untitled", query)}
                </h3>
                <p className="flex gap-x-3 text-xs text-muted-foreground">
                    <span className="font-bold">{item.feed_title}</span>
                    {item.published_at && relativeTimeLong(item.published_at)}
                </p>
                {item.snippet && (
                    <p className="line-clamp-2 text-sm text-muted-foreground">
                        {highlight(item.snippet, query)}
                    </p>
                )}

                {/* Pills */}
                <div className="mt-auto flex flex-wrap items-center gap-1.5 pt-1">
                    {timeLabel && (
                        <Badge variant="secondary">{timeLabel}</Badge>
                    )}
                    <Badge
                        variant="secondary"
                        className={cn(topicBadgeClass(item.category))}
                    >
                        {item.category}
                    </Badge>
                    {item.score != null && (
                        <Badge variant="secondary" className="gap-1">
                            <TrendingUp className="size-3" /> {item.score}
                        </Badge>
                    )}
                    {item.comments_count != null && (
                        <Badge variant="secondary" className="gap-1">
                            <MessageSquare className="size-3" />{" "}
                            {item.comments_count}
                        </Badge>
                    )}
                </div>
            </div>
        </button>
    );
}
