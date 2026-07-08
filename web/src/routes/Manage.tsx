import { useMemo, useState } from "react";
import { FolderPlus, Pencil, Plus, RefreshCw, Rss, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { EmptyState } from "@/components/common/EmptyState";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { FeedEditModal } from "@/components/feeds/FeedEditModal";
import { useCategories, useCreateCategory, useDeleteCategory, useUpdateCategory } from "@/hooks/useCategories";
import { useFeeds, useRefreshFeed, useUnsubscribe } from "@/hooks/useFeeds";
import { kindIcon, kindLabel } from "@/lib/format";
import type { Category, Feed } from "@/lib/types";
import { useUiStore } from "@/stores/ui";
import { toast } from "@/stores/toast";

/** Manage categories & feeds — the structural hub (prompt.md §9.5). */
export function Manage() {
  const feeds = useFeeds();
  const categories = useCategories();
  const openAddFeed = useUiStore((s) => s.setAddFeedOpen);

  const [categoryFilter, setCategoryFilter] = useState<number | "all">("all");
  const [search, setSearch] = useState("");
  const [editing, setEditing] = useState<Feed | null>(null);

  const createCategory = useCreateCategory();

  const grouped = useMemo(() => groupByCategory(categories.data ?? [], feeds.data ?? []), [categories.data, feeds.data]);

  const visible = grouped
    .filter((g) => categoryFilter === "all" || g.category.id === categoryFilter)
    .map((g) => ({
      ...g,
      feeds: g.feeds.filter((f) => f.title.toLowerCase().includes(search.trim().toLowerCase())),
    }));

  const newCategory = () => {
    const name = window.prompt("New category name")?.trim();
    if (!name) return;
    createCategory.mutate(name, {
      onSuccess: () => toast("Category created", "success"),
      onError: (e) => toast(e instanceof Error ? e.message : "Could not create", "error"),
    });
  };

  const isLoading = feeds.isLoading || categories.isLoading;
  const isError = feeds.isError || categories.isError;
  const noFeeds = (feeds.data?.length ?? 0) === 0;

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-2xl font-bold">Manage feeds</h1>
        <div className="flex gap-2">
          <Button onClick={() => openAddFeed(true)}>
            <Plus className="size-4" /> Add feed
          </Button>
          <Button variant="outline" onClick={newCategory}>
            <FolderPlus className="size-4" /> New category
          </Button>
        </div>
      </div>

      {!noFeeds && (
        <div className="flex flex-wrap gap-3">
          <Select
            className="max-w-xs"
            value={categoryFilter}
            onChange={(e) => setCategoryFilter(e.target.value === "all" ? "all" : Number(e.target.value))}
          >
            <option value="all">All categories</option>
            {categories.data?.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name} ({c.feed_count})
              </option>
            ))}
          </Select>
          <Input
            className="max-w-xs"
            placeholder="Search feeds by name…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
      )}

      {isError && <ErrorBanner error={feeds.error ?? categories.error} />}

      {isLoading && (
        <div className="space-y-3">
          {[0, 1].map((i) => (
            <Skeleton key={i} className="h-40 w-full" />
          ))}
        </div>
      )}

      {!isLoading && !isError && noFeeds && (
        <EmptyState
          icon={<Rss className="size-8" />}
          title="No feeds yet"
          description="Add your first feed to start building your digestly."
          action={
            <Button onClick={() => openAddFeed(true)}>
              <Plus className="size-4" /> Add your first feed
            </Button>
          }
        />
      )}

      {!isLoading &&
        !isError &&
        !noFeeds &&
        visible.map((g) => (
          <CategoryCard key={g.category.id} category={g.category} feeds={g.feeds} onEdit={setEditing} />
        ))}

      <FeedEditModal feed={editing} onClose={() => setEditing(null)} />
    </div>
  );
}

function groupByCategory(categories: Category[], feeds: Feed[]) {
  return categories.map((category) => ({
    category,
    feeds: feeds.filter((f) => f.category_id === category.id),
  }));
}

function CategoryCard({
  category,
  feeds,
  onEdit,
}: {
  category: Category;
  feeds: Feed[];
  onEdit: (f: Feed) => void;
}) {
  const rename = useUpdateCategory();
  const del = useDeleteCategory();
  const openAddFeed = useUiStore((s) => s.setAddFeedOpen);

  const doRename = () => {
    const name = window.prompt("Rename category", category.name)?.trim();
    if (!name || name === category.name) return;
    rename.mutate(
      { id: category.id, name },
      {
        onSuccess: () => toast("Category renamed", "success"),
        onError: (e) => toast(e instanceof Error ? e.message : "Could not rename", "error"),
      },
    );
  };

  const doDelete = () => {
    if (!confirm(`Delete "${category.name}"? Its feeds move to Other.`)) return;
    del.mutate(category.id, {
      onSuccess: () => toast("Category deleted; feeds moved to Other", "success"),
      onError: (e) => toast(e instanceof Error ? e.message : "Could not delete", "error"),
    });
  };

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between gap-2 space-y-0">
        <div className="flex items-baseline gap-2">
          <h2 className="text-lg font-semibold">{category.name}</h2>
          <span className="text-sm text-muted-foreground">{feeds.length}</span>
        </div>
        <div className="flex gap-1">
          <Button variant="ghost" size="sm" onClick={() => openAddFeed(true)}>
            <Plus className="size-4" />
          </Button>
          {category.deletable && (
            <Button variant="ghost" size="sm" onClick={doRename} aria-label="Rename">
              <Pencil className="size-4" />
            </Button>
          )}
          {category.deletable && (
            <Button variant="ghost" size="sm" onClick={doDelete} aria-label="Delete category">
              <Trash2 className="size-4" />
            </Button>
          )}
        </div>
      </CardHeader>
      <CardContent>
        {feeds.length === 0 ? (
          <p className="text-sm text-muted-foreground">No feeds in this category.</p>
        ) : (
          <ul className="divide-y divide-border">
            {feeds.map((f) => (
              <FeedRow key={f.id} feed={f} onEdit={onEdit} />
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}

function FeedRow({ feed, onEdit }: { feed: Feed; onEdit: (f: Feed) => void }) {
  const refresh = useRefreshFeed();
  const unsubscribe = useUnsubscribe();

  const meta = [
    kindLabel(feed.kind),
    feed.content_type === "video" ? "🎬 video" : "📖 reading",
    `every ${Math.round(feed.fetch_interval_secs / 60)}m`,
    `${feed.item_count} items`,
  ].join(" · ");

  return (
    <li className="flex items-center gap-3 py-2.5">
      <span className="text-lg" title={kindLabel(feed.kind)}>
        {kindIcon(feed.kind)}
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate text-sm font-medium">{feed.title}</span>
          {feed.disabled && <Badge variant="secondary">paused</Badge>}
          {feed.feed_disabled && <Badge variant="destructive">disabled</Badge>}
          {feed.kind === "reddit" && <Badge variant="outline">min ▲ {feed.min_score}</Badge>}
        </div>
        <p className="truncate text-xs text-muted-foreground">{meta}</p>
      </div>
      <div className="flex shrink-0 gap-1">
        <Button variant="ghost" size="icon" aria-label="Refresh" onClick={() => refresh.mutate(feed.id, { onSuccess: () => toast("Refreshing…") })}>
          <RefreshCw className="size-4" />
        </Button>
        <Button variant="ghost" size="icon" aria-label="Edit" onClick={() => onEdit(feed)}>
          <Pencil className="size-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          aria-label="Unsubscribe"
          onClick={() => {
            if (confirm(`Unsubscribe from "${feed.title}"?`)) {
              unsubscribe.mutate(feed.id, { onSuccess: () => toast("Unsubscribed", "success") });
            }
          }}
        >
          <Trash2 className="size-4" />
        </Button>
      </div>
    </li>
  );
}
