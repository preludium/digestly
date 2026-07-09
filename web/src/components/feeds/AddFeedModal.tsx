import { useState } from "react";
import { Plus, Search } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { NumberField } from "@/components/ui/number-field";
import { Spinner } from "@/components/ui/spinner";
import { Badge } from "@/components/ui/badge";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { useCategories, useCreateCategory } from "@/hooks/useCategories";
import { useDiscover, useSubscribe } from "@/hooks/useFeeds";
import { kindIcon, kindLabel } from "@/lib/format";
import type { ContentType, DiscoverCandidate } from "@/lib/types";
import { useUiStore } from "@/stores/ui";
import { toast } from "@/stores/toast";

/** Add / subscribe flow (prompt.md §9.3): discover → pick → required category → subscribe. */
export function AddFeedModal() {
  const open = useUiStore((s) => s.addFeedOpen);
  const setOpen = useUiStore((s) => s.setAddFeedOpen);

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Add a feed</DialogTitle>
          <DialogDescription>
            Paste a site URL, feed URL, YouTube channel/@handle, or subreddit.
          </DialogDescription>
        </DialogHeader>
        <AddFeedBody onDone={() => setOpen(false)} />
      </DialogContent>
    </Dialog>
  );
}

function AddFeedBody({ onDone }: { onDone: () => void }) {
  const [input, setInput] = useState("");
  const [selected, setSelected] = useState<DiscoverCandidate | null>(null);

  const discover = useDiscover();
  const candidates = discover.data;

  const runDiscover = () => {
    if (!input.trim()) return;
    setSelected(null);
    discover.mutate(input.trim(), {
      onSuccess: (list) => {
        // Auto-select the sole subscribable candidate for a one-tap add.
        const free = list.filter((c) => !c.already_subscribed);
        if (free.length === 1) setSelected(free[0]);
      },
    });
  };

  // Fallback: treat the raw input as a direct feed URL when discovery finds nothing.
  const useRawUrl = () => {
    if (!/^https?:\/\//i.test(input.trim())) {
      toast("Enter a full http(s):// feed URL", "error");
      return;
    }
    setSelected({
      feed_url: input.trim(),
      title: null,
      kind: "rss",
      site_url: null,
      icon_url: null,
      already_subscribed: false,
    });
  };

  return (
    <div className="space-y-4">
      <form
        className="flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          runDiscover();
        }}
      >
        <Input
          autoFocus
          placeholder="Feed or site URL"
          value={input}
          onChange={(e) => setInput(e.target.value)}
        />
        <Button type="submit" disabled={discover.isPending || !input.trim()}>
          {discover.isPending ? <Spinner className="size-4" /> : <Search className="size-4" />}
          <span className="hidden sm:inline">Find</span>
        </Button>
      </form>

      {discover.isError && <ErrorBanner error={discover.error} />}

      {candidates && candidates.length === 0 && !selected && (
        <div className="space-y-2 rounded-md border border-dashed border-border p-4 text-sm text-muted-foreground">
          <p>No feeds found for that input.</p>
          <Button variant="outline" size="sm" onClick={useRawUrl}>
            Use this URL as a feed directly
          </Button>
        </div>
      )}

      {candidates && candidates.length > 0 && !selected && (
        <ul className="space-y-2">
          {candidates.map((c) => (
            <li key={c.feed_url}>
              <button
                type="button"
                disabled={c.already_subscribed}
                onClick={() => setSelected(c)}
                className="flex w-full items-center gap-3 rounded-md border border-border p-3 text-left text-sm transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
              >
                <span className="text-lg">{kindIcon(c.kind)}</span>
                <span className="min-w-0 flex-1">
                  <span className="block truncate font-medium">{c.title ?? c.feed_url}</span>
                  <span className="block truncate text-xs text-muted-foreground">{c.feed_url}</span>
                </span>
                {c.already_subscribed ? (
                  <Badge variant="secondary">subscribed</Badge>
                ) : (
                  <Badge variant="outline">{kindLabel(c.kind)}</Badge>
                )}
              </button>
            </li>
          ))}
        </ul>
      )}

      {selected && (
        <ConfigureStep candidate={selected} onBack={() => setSelected(null)} onDone={onDone} />
      )}
    </div>
  );
}

function ConfigureStep({
  candidate,
  onBack,
  onDone,
}: {
  candidate: DiscoverCandidate;
  onBack: () => void;
  onDone: () => void;
}) {
  const categories = useCategories();
  const createCategory = useCreateCategory();
  const subscribe = useSubscribe();

  const [categoryId, setCategoryId] = useState<number | "">("");
  const [creatingNew, setCreatingNew] = useState(false);
  const [newCategory, setNewCategory] = useState("");
  const [contentType, setContentType] = useState<ContentType>(candidate.kind === "youtube" ? "video" : "reading");
  const [minScore, setMinScore] = useState(candidate.kind === "reddit" ? 50 : 0);
  const [interval, setInterval] = useState(60);
  const [fullText, setFullText] = useState(false);
  const [showAdvanced, setShowAdvanced] = useState(false);

  const addNewCategory = () => {
    const name = newCategory.trim();
    if (!name) return;
    createCategory.mutate(name, {
      onSuccess: (cat) => {
        setCategoryId(cat.id);
        setCreatingNew(false);
        setNewCategory("");
      },
      onError: (e) => toast(e instanceof Error ? e.message : "Could not create category", "error"),
    });
  };

  const submit = () => {
    if (categoryId === "") {
      toast("Choose a category first", "error");
      return;
    }
    subscribe.mutate(
      {
        feed_url: candidate.feed_url,
        kind: candidate.kind,
        title: candidate.title,
        site_url: candidate.site_url,
        category_id: categoryId,
        content_type: contentType,
        min_score: minScore,
        full_text_extract: fullText,
        fetch_interval_secs: interval * 60,
      },
      {
        onSuccess: () => {
          toast("Feed added", "success");
          onDone();
        },
        onError: (e) => toast(e instanceof Error ? e.message : "Could not subscribe", "error"),
      },
    );
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-3 rounded-md border border-border p-3 text-sm">
        <span className="text-lg">{kindIcon(candidate.kind)}</span>
        <span className="min-w-0 flex-1 truncate font-medium">{candidate.title ?? candidate.feed_url}</span>
        <Button variant="ghost" size="sm" onClick={onBack}>
          Change
        </Button>
      </div>

      {/* Category — REQUIRED (§9.3). */}
      <div className="space-y-1.5">
        <Label htmlFor="category">
          Category <span className="text-destructive">*</span>
        </Label>
        {creatingNew ? (
          <div className="flex gap-2">
            <Input
              autoFocus
              placeholder="New category name"
              value={newCategory}
              onChange={(e) => setNewCategory(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && (e.preventDefault(), addNewCategory())}
            />
            <Button type="button" onClick={addNewCategory} disabled={createCategory.isPending}>
              Add
            </Button>
            <Button type="button" variant="ghost" onClick={() => setCreatingNew(false)}>
              Cancel
            </Button>
          </div>
        ) : (
          <div className="flex gap-2">
            <Select
              value={categoryId === "" ? "" : String(categoryId)}
              onValueChange={(v) => setCategoryId(v ? Number(v) : "")}
            >
              <SelectTrigger id="category">
                <SelectValue placeholder="Select a category…" />
              </SelectTrigger>
              <SelectContent>
                {categories.data?.map((c) => (
                  <SelectItem key={c.id} value={String(c.id)}>
                    {c.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button type="button" variant="outline" onClick={() => setCreatingNew(true)}>
              <Plus className="size-4" /> New
            </Button>
          </div>
        )}
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="content-type">Content type</Label>
        <Select value={contentType} onValueChange={(v) => setContentType(v as ContentType)}>
          <SelectTrigger id="content-type">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="reading">📖 Reading</SelectItem>
            <SelectItem value="video">🎬 Video</SelectItem>
          </SelectContent>
        </Select>
      </div>

      {candidate.kind === "reddit" && (
        <div className="space-y-1.5">
          <Label htmlFor="min-score">Minimum score (Reddit)</Label>
          <NumberField
            id="min-score"
            value={minScore}
            onChange={setMinScore}
            min={0}
          />
        </div>
      )}

      <button
        type="button"
        className="text-sm font-medium text-muted-foreground hover:text-foreground"
        onClick={() => setShowAdvanced((v) => !v)}
      >
        {showAdvanced ? "− Hide advanced" : "+ Advanced"}
      </button>

      {showAdvanced && (
        <div className="space-y-4 rounded-md border border-border p-3">
          <div className="space-y-1.5">
            <Label htmlFor="interval">Fetch interval (minutes)</Label>
            <NumberField
              id="interval"
              value={interval}
              onChange={setInterval}
              min={1}
            />
          </div>
          <div className="flex items-center justify-between gap-3">
            <Label htmlFor="fulltext" className="text-sm font-normal">Fetch full article text (readability)</Label>
            <Switch id="fulltext" checked={fullText} onCheckedChange={setFullText} />
          </div>
        </div>
      )}

      <div className="flex justify-end gap-2 pt-2">
        <Button type="button" onClick={submit} disabled={subscribe.isPending || categoryId === ""}>
          {subscribe.isPending ? "Adding…" : "Add feed"}
        </Button>
      </div>
    </div>
  );
}
