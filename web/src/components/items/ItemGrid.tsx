import { CardGridSkeleton } from "@/components/common/LoadingSkeleton";
import { EmptyState } from "@/components/common/EmptyState";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { Pagination } from "@/components/common/Pagination";
import { ItemCard } from "@/components/items/ItemCard";
import { sortLabel } from "@/components/items/FilterBar";
import type { FeedFilters } from "@/hooks/useFeedFilters";
import type { Item, ItemsPage } from "@/lib/types";

interface ItemGridProps {
  data: ItemsPage | undefined;
  isLoading: boolean;
  isError: boolean;
  error: unknown;
  filters: FeedFilters;
  onOpen: (item: Item) => void;
  onPage: (page: number) => void;
  emptyTitle: string;
  emptyDescription?: string;
}

/** Result line + responsive 1→4 card grid + numbered pagination, with loading/empty/error states.
 *  Shared verbatim by the feed and search screens (§9.1, §9.2 — DRY). */
export function ItemGrid({
  data,
  isLoading,
  isError,
  error,
  filters,
  onOpen,
  onPage,
  emptyTitle,
  emptyDescription,
}: ItemGridProps) {
  if (isError) return <ErrorBanner error={error} />;
  if (isLoading || !data) return <CardGridSkeleton />;

  if (data.total_count === 0) {
    return <EmptyState title={emptyTitle} description={emptyDescription} />;
  }

  return (
    <div className="space-y-4">
      <p className="text-sm text-muted-foreground">
        {data.total_count} {data.total_count === 1 ? "item" : "items"} · sorted by {sortLabel(filters.sort)}
      </p>

      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 wide:grid-cols-3 xl:grid-cols-4">
        {data.items.map((item) => (
          <ItemCard key={item.id} item={item} onOpen={onOpen} query={filters.q} />
        ))}
      </div>

      <Pagination page={data.page} totalPages={data.total_pages} onPage={onPage} />
    </div>
  );
}
