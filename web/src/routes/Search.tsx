import { useEffect, useState } from "react";
import { Search as SearchIcon } from "lucide-react";
import { Input } from "@/components/ui/input";
import { EmptyState } from "@/components/common/EmptyState";
import { FilterBar } from "@/components/items/FilterBar";
import { ItemGrid } from "@/components/items/ItemGrid";
import { ItemPreview } from "@/components/items/ItemPreview";
import { useFeedFilters } from "@/hooks/useFeedFilters";
import { useItems } from "@/hooks/useItems";
import type { Item } from "@/lib/types";

/** Search screen (§9.2). Reuses the grid + pagination + facets; debounced input, term highlight. */
export function Search() {
  const { filters, setFacet, setPage, clear } = useFeedFilters(true);
  const [text, setText] = useState(filters.q);
  const [preview, setPreview] = useState<Item | null>(null);

  const hasQuery = filters.q.trim().length > 0;
  const items = useItems(filters, hasQuery);

  // Debounce input → URL query (source of truth). Resets to page 1 via setFacet.
  useEffect(() => {
    if (text === filters.q) return;
    const id = setTimeout(() => setFacet("q", text), 300);
    return () => clearTimeout(id);
  }, [text, filters.q, setFacet]);

  return (
    <div className="space-y-4">
      <h1 className="font-display text-2xl font-semibold tracking-tight">Search</h1>

      <div className="relative">
        <SearchIcon className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input
          autoFocus
          className="pl-9"
          placeholder="Search titles and content…"
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
      </div>

      <FilterBar filters={filters} setFacet={setFacet} clear={clear} />

      {!hasQuery ? (
        <EmptyState icon={<SearchIcon className="size-8" />} title="Type to search" description="Search across your feeds' titles and content." />
      ) : (
        <ItemGrid
          data={items.data}
          isLoading={items.isLoading}
          isError={items.isError}
          error={items.error}
          filters={filters}
          onOpen={setPreview}
          onPage={setPage}
          emptyTitle={`No results for “${filters.q}”`}
          emptyDescription="Try different keywords or clear the filters."
        />
      )}

      <ItemPreview item={preview} onClose={() => setPreview(null)} />
    </div>
  );
}
