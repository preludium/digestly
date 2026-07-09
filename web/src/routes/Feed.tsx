import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Rss } from "lucide-react";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/common/EmptyState";
import { FilterBar } from "@/components/items/FilterBar";
import { ItemGrid } from "@/components/items/ItemGrid";
import { ItemPreview } from "@/components/items/ItemPreview";
import { useFeedFilters } from "@/hooks/useFeedFilters";
import { useItems, useToggleRead, useToggleStar } from "@/hooks/useItems";
import { useFeeds, useRefreshAll } from "@/hooks/useFeeds";
import { useUiStore } from "@/stores/ui";
import { toast } from "@/stores/toast";
import type { Item } from "@/lib/types";

/** The core feed screen — responsive card grid with unified filters (prompt.md §9.1). */
export function Feed() {
  const { filters, setFacet, setPage, clear } = useFeedFilters(false);
  const items = useItems(filters);
  const feeds = useFeeds();
  const refreshAll = useRefreshAll();
  const toggleRead = useToggleRead();
  const toggleStar = useToggleStar();
  const openAddFeed = useUiStore((s) => s.setAddFeedOpen);
  const navigate = useNavigate();

  const [preview, setPreview] = useState<Item | null>(null);

  // Keyboard shortcuts (§9.1). Ignored while typing in a form control.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable)) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      switch (e.key) {
        case "n":
          if (items.data && filters.page < items.data.total_pages) setPage(filters.page + 1);
          break;
        case "p":
          if (filters.page > 1) setPage(filters.page - 1);
          break;
        case "r":
          e.preventDefault();
          refreshAll.mutate(undefined, { onSuccess: () => toast("Refreshing feeds…") });
          break;
        case "/":
          e.preventDefault();
          navigate("/search");
          break;
        case "o":
          if (preview?.url) window.open(preview.url, "_blank", "noopener");
          break;
        case "m":
          if (preview) toggleRead.mutate({ id: preview.id, value: !preview.is_read });
          break;
        case "s":
          if (preview) toggleStar.mutate({ id: preview.id, value: !preview.is_starred });
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [items.data, filters.page, preview, setPage, refreshAll, navigate, toggleRead, toggleStar]);

  // First-run: no subscriptions yet.
  if (feeds.data && feeds.data.length === 0) {
    return (
      <EmptyState
        icon={<Rss className="size-8" />}
        title="No feeds yet"
        description="Add your first feed to start building your digestly."
        action={<Button onClick={() => openAddFeed(true)}>Add your first feed</Button>}
      />
    );
  }

  return (
    <div className="space-y-4">
      <h1 className="font-display text-2xl font-semibold tracking-tight">Your feed</h1>
      <FilterBar filters={filters} setFacet={setFacet} clear={clear} />
      <ItemGrid
        data={items.data}
        isLoading={items.isLoading}
        isError={items.isError}
        error={items.error}
        filters={filters}
        onOpen={setPreview}
        onPage={setPage}
        emptyTitle="Nothing matches these filters 🎉"
        emptyDescription="Try clearing a filter or widening the time range."
      />
      <ItemPreview item={preview} onClose={() => setPreview(null)} />
    </div>
  );
}
