import { useEffect, useState } from "react";
import { RefreshCw, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select } from "@/components/ui/select";
import { useCategories } from "@/hooks/useCategories";
import { useRefreshFeed, useUnsubscribe, useUpdateFeed } from "@/hooks/useFeeds";
import { formatDateTime, kindLabel } from "@/lib/format";
import type { ContentType, Feed } from "@/lib/types";
import { toast } from "@/stores/toast";

/** Feed settings / edit (prompt.md §9.4). Controlled by the caller; `feed` null hides it. */
export function FeedEditModal({ feed, onClose }: { feed: Feed | null; onClose: () => void }) {
  return (
    <Dialog open={!!feed} onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Edit feed</DialogTitle>
        </DialogHeader>
        {feed && <EditBody key={feed.id} feed={feed} onClose={onClose} />}
      </DialogContent>
    </Dialog>
  );
}

function EditBody({ feed, onClose }: { feed: Feed; onClose: () => void }) {
  const categories = useCategories();
  const update = useUpdateFeed();
  const refresh = useRefreshFeed();
  const unsubscribe = useUnsubscribe();

  const [titleOverride, setTitleOverride] = useState(feed.title);
  const [categoryId, setCategoryId] = useState(feed.category_id);
  const [contentType, setContentType] = useState<ContentType>(feed.content_type);
  const [minScore, setMinScore] = useState(feed.min_score);
  const [fullText, setFullText] = useState(feed.full_text_extract);
  const [disabled, setDisabled] = useState(feed.disabled);
  const [intervalMin, setIntervalMin] = useState(Math.round(feed.fetch_interval_secs / 60));

  // Reset local state if a different feed is opened (belt-and-suspenders with the `key` above).
  useEffect(() => {
    setTitleOverride(feed.title);
    setCategoryId(feed.category_id);
  }, [feed.id, feed.title, feed.category_id]);

  const save = () => {
    update.mutate(
      {
        id: feed.id,
        title_override: titleOverride,
        category_id: categoryId,
        content_type: contentType,
        min_score: minScore,
        full_text_extract: fullText,
        disabled,
        fetch_interval_secs: intervalMin * 60,
      },
      {
        onSuccess: () => {
          toast("Feed updated", "success");
          onClose();
        },
        onError: (e) => toast(e instanceof Error ? e.message : "Could not save", "error"),
      },
    );
  };

  return (
    <div className="space-y-4">
      <div className="space-y-1.5">
        <Label htmlFor="title">Title override</Label>
        <Input id="title" value={titleOverride} onChange={(e) => setTitleOverride(e.target.value)} />
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="cat">
          Category <span className="text-destructive">*</span>
        </Label>
        <Select id="cat" value={categoryId} onChange={(e) => setCategoryId(Number(e.target.value))}>
          {categories.data?.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </Select>
      </div>

      <div className="grid grid-cols-2 gap-3">
        <div className="space-y-1.5">
          <Label htmlFor="ct">Content type</Label>
          <Select id="ct" value={contentType} onChange={(e) => setContentType(e.target.value as ContentType)}>
            <option value="reading">📖 Reading</option>
            <option value="video">🎬 Video</option>
          </Select>
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="int">Interval (min)</Label>
          <Input
            id="int"
            type="number"
            min={1}
            value={intervalMin}
            onChange={(e) => setIntervalMin(Math.max(1, Number(e.target.value) || 60))}
          />
        </div>
      </div>

      {feed.kind === "reddit" && (
        <div className="space-y-1.5">
          <Label htmlFor="ms">Minimum score (Reddit)</Label>
          <Input
            id="ms"
            type="number"
            min={0}
            value={minScore}
            onChange={(e) => setMinScore(Math.max(0, Number(e.target.value) || 0))}
          />
        </div>
      )}

      <div className="space-y-2">
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" className="size-4 accent-primary" checked={fullText} onChange={(e) => setFullText(e.target.checked)} />
          Fetch full article text (readability)
        </label>
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" className="size-4 accent-primary" checked={disabled} onChange={(e) => setDisabled(e.target.checked)} />
          Pause this feed (stop showing new items)
        </label>
      </div>

      <div className="space-y-1.5">
        <Label>Feed URL</Label>
        <Input value={feed.feed_url} readOnly className="text-muted-foreground" />
      </div>

      {/* Diagnostics (§9.4). */}
      <dl className="grid grid-cols-2 gap-x-4 gap-y-1 rounded-md border border-border p-3 text-xs text-muted-foreground">
        <dt>Kind</dt>
        <dd className="text-right text-foreground">{kindLabel(feed.kind)}</dd>
        <dt>Items stored</dt>
        <dd className="text-right text-foreground">{feed.item_count}</dd>
        <dt>Last fetch</dt>
        <dd className="text-right text-foreground">{formatDateTime(feed.last_fetch_at)}</dd>
        <dt>Failures</dt>
        <dd className="text-right text-foreground">{feed.failure_count}</dd>
        {feed.last_error && (
          <>
            <dt>Last error</dt>
            <dd className="text-right text-destructive">{feed.last_error}</dd>
          </>
        )}
      </dl>

      <div className="flex flex-wrap items-center justify-between gap-2 pt-2">
        <div className="flex gap-2">
          <Button
            type="button"
            variant="outline"
            onClick={() => refresh.mutate(feed.id, { onSuccess: () => toast("Refreshing…") })}
            disabled={refresh.isPending}
          >
            <RefreshCw className="size-4" /> Refresh now
          </Button>
          <Button
            type="button"
            variant="destructive"
            onClick={() => {
              if (confirm(`Unsubscribe from "${feed.title}"?`)) {
                unsubscribe.mutate(feed.id, {
                  onSuccess: () => {
                    toast("Unsubscribed", "success");
                    onClose();
                  },
                });
              }
            }}
            disabled={unsubscribe.isPending}
          >
            <Trash2 className="size-4" /> Unsubscribe
          </Button>
        </div>
        <Button type="button" onClick={save} disabled={update.isPending}>
          {update.isPending ? "Saving…" : "Save"}
        </Button>
      </div>
    </div>
  );
}
