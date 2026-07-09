import { useEffect, useState } from "react";
import { RefreshCw, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { NumberField } from "@/components/ui/number-field";
import { Switch } from "@/components/ui/switch";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { useCategories } from "@/hooks/useCategories";
import { useRefreshFeed, useUnsubscribe, useUpdateFeed } from "@/hooks/useFeeds";
import { formatDateTime, kindLabel } from "@/lib/format";
import type { ContentType, Feed } from "@/lib/types";
import { toast } from "@/stores/toast";
import { sortCategoriesOtherLast } from "@/routes/manage.helpers";

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
  const [unsubscribing, setUnsubscribing] = useState(false);

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
      {/* Basics */}
      <p className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Basics</p>
      <div className="space-y-1.5">
        <Label htmlFor="title">Title override</Label>
        <Input id="title" value={titleOverride} onChange={(e) => setTitleOverride(e.target.value)} />
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="cat">
          Category <span className="text-destructive">*</span>
        </Label>
        <Select value={String(categoryId)} onValueChange={(v) => setCategoryId(Number(v))}>
          <SelectTrigger id="cat">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {sortCategoriesOtherLast(categories.data ?? []).map((c) => (
              <SelectItem key={c.id} value={String(c.id)}>
                {c.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="border-t border-border" />

      {/* Fetching */}
      <p className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Fetching</p>
      <div className="grid grid-cols-2 gap-3">
        <div className="space-y-1.5">
          <Label htmlFor="ct">Content type</Label>
          <Select value={contentType} onValueChange={(v) => setContentType(v as ContentType)}>
            <SelectTrigger id="ct">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="reading">📖 Reading</SelectItem>
              <SelectItem value="video">🎬 Video</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="int">Interval (min)</Label>
          <NumberField
            id="int"
            value={intervalMin}
            onChange={setIntervalMin}
            min={1}
          />
        </div>
      </div>

      {feed.kind === "reddit" && (
        <div className="space-y-1.5">
          <Label htmlFor="ms">Minimum score (Reddit)</Label>
          <NumberField
            id="ms"
            value={minScore}
            onChange={setMinScore}
            min={0}
          />
        </div>
      )}

      <div className="space-y-3">
        <div className="flex items-center justify-between gap-3">
          <Label htmlFor="fulltext" className="text-sm font-normal">Fetch full article text (readability)</Label>
          <Switch id="fulltext" checked={fullText} onCheckedChange={setFullText} />
        </div>
        <div className="flex items-center justify-between gap-3">
          <Label htmlFor="paused" className="text-sm font-normal">Pause this feed (stop showing new items)</Label>
          <Switch id="paused" checked={disabled} onCheckedChange={setDisabled} />
        </div>
      </div>

      <div className="border-t border-border" />

      {/* Diagnostics */}
      <p className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Diagnostics</p>
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
            onClick={() => setUnsubscribing(true)}
            disabled={unsubscribe.isPending}
          >
            <Trash2 className="size-4" /> Unsubscribe
          </Button>
          <ConfirmDialog
            open={unsubscribing}
            onOpenChange={setUnsubscribing}
            title={`Unsubscribe from "${feed.title}"?`}
            confirmLabel="Unsubscribe"
            destructive
            onConfirm={() => {
              unsubscribe.mutate(feed.id, {
                onSuccess: () => {
                  toast("Unsubscribed", "success");
                  onClose();
                },
              });
            }}
          />
        </div>
        <Button type="button" onClick={save} disabled={update.isPending}>
          {update.isPending ? "Saving…" : "Save"}
        </Button>
      </div>
    </div>
  );
}
