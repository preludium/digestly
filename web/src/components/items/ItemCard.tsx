import { ImageIcon, MessageSquare, Play, Star, TrendingUp } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import { formatDuration, readingTimeLabel, relativeTime } from "@/lib/format";
import { highlight } from "@/lib/highlight";
import { topicBadgeClass } from "@/lib/topicColor";
import type { Item } from "@/lib/types";

/** The single card used by both the feed and search grids (prompt.md §9.1 — DRY, one card). */
export function ItemCard({ item, onOpen, query = "" }: { item: Item; onOpen: (item: Item) => void; query?: string }) {
  const isVideo = item.content_type === "video";
  const timeLabel = readingTimeLabel(item.reading_time_secs, item.content_type);
  const duration = formatDuration(item.duration_secs);

  return (
    <button
      type="button"
      onClick={() => onOpen(item)}
      className={cn(
        "group flex flex-col overflow-hidden rounded-lg border border-border bg-card text-left transition-colors hover:border-primary/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        item.is_read && "opacity-60",
      )}
    >
      {/* Thumbnail */}
      <div className="relative aspect-video w-full overflow-hidden bg-muted">
        {item.image_url ? (
          <img src={item.image_url} alt="" loading="lazy" className="size-full object-cover" />
        ) : (
          <div className="flex size-full items-center justify-center text-muted-foreground">
            <ImageIcon className="size-8" />
          </div>
        )}
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
        <h3 className={cn("line-clamp-2 font-semibold leading-snug", item.is_read && "text-muted-foreground")}>{highlight(item.title ?? "Untitled", query)}</h3>
        <p className="text-xs text-muted-foreground">
          <span className="font-medium">{item.feed_title}</span>
          {item.published_at && <> · {relativeTime(item.published_at)}</>}
        </p>
        {item.snippet && <p className="line-clamp-2 text-sm text-muted-foreground">{highlight(item.snippet, query)}</p>}

        {/* Pills */}
        <div className="mt-auto flex flex-wrap items-center gap-1.5 pt-1">
          {timeLabel && <Badge variant="secondary">{timeLabel}</Badge>}
          <Badge variant="secondary" className={cn(topicBadgeClass(item.category))}>{item.category}</Badge>
          {item.score != null && (
            <Badge variant="secondary" className="gap-1">
              <TrendingUp className="size-3" /> {item.score}
            </Badge>
          )}
          {item.comments_count != null && (
            <Badge variant="secondary" className="gap-1">
              <MessageSquare className="size-3" /> {item.comments_count}
            </Badge>
          )}
        </div>
      </div>
    </button>
  );
}
