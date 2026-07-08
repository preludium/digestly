import { useState } from "react";
import { SlidersHorizontal, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { Sheet, SheetContent, SheetTitle } from "@/components/ui/sheet";
import { cn } from "@/lib/utils";
import { useCategories } from "@/hooks/useCategories";
import { useCategoryCounts } from "@/hooks/useItems";
import { activeFilterCount, type FeedFilters } from "@/hooks/useFeedFilters";
import type { ItemSort, ItemStatus, ItemType, ItemWhen } from "@/lib/types";

const TYPES: { value: ItemType; label: string }[] = [
  { value: "all", label: "All" },
  { value: "reading", label: "📖 Reading" },
  { value: "video", label: "🎬 Videos" },
];
const STATUSES: { value: ItemStatus; label: string }[] = [
  { value: "all", label: "All" },
  { value: "unread", label: "Unread" },
  { value: "starred", label: "★ Starred" },
];
const WHENS: { value: ItemWhen; label: string }[] = [
  { value: "all", label: "All time" },
  { value: "today", label: "Today" },
  { value: "yesterday", label: "Yesterday" },
  { value: "week", label: "This week" },
  { value: "month", label: "This month" },
];
const SORTS: { value: ItemSort; label: string }[] = [
  { value: "new", label: "Newest" },
  { value: "old", label: "Oldest" },
  { value: "quick", label: "Quickest read" },
  { value: "top", label: "Most popular" },
  { value: "discussed", label: "Most discussed" },
  { value: "unread", label: "Unread first" },
];

export function sortLabel(sort: ItemSort): string {
  return SORTS.find((s) => s.value === sort)?.label ?? "Newest";
}

interface FilterBarProps {
  filters: FeedFilters;
  setFacet: <K extends keyof FeedFilters>(key: K, value: FeedFilters[K]) => void;
  clear: () => void;
}

/** Unified, combinable filter bar (§9.1). Inline at ≥820px; collapses behind a ⚙ Filters button
 *  (with count badge + summary + Clear) below. One implementation powers the feed and search. */
export function FilterBar({ filters, setFacet, clear }: FilterBarProps) {
  const [open, setOpen] = useState(false);
  const activeCount = activeFilterCount(filters);

  return (
    <div className="space-y-3">
      {/* Inline (desktop ≥820px) */}
      <div className="hidden wide:block">
        <Facets filters={filters} setFacet={setFacet} clear={clear} activeCount={activeCount} />
      </div>

      {/* Collapsed trigger (mobile <820px) */}
      <div className="flex items-center gap-2 wide:hidden">
        <Button variant="outline" size="sm" onClick={() => setOpen(true)}>
          <SlidersHorizontal className="size-4" />
          Filters
          {activeCount > 0 && (
            <span className="ml-1 flex size-5 items-center justify-center rounded-full bg-primary text-xs font-semibold text-primary-foreground">
              {activeCount}
            </span>
          )}
        </Button>
        <p className="min-w-0 flex-1 truncate text-sm text-muted-foreground">{summary(filters)}</p>
        {activeCount > 0 && (
          <Button variant="ghost" size="sm" onClick={clear}>
            Clear
          </Button>
        )}
      </div>

      {/* Mobile filter panel */}
      <Sheet open={open} onOpenChange={setOpen}>
        <SheetContent side="bottom" className="max-h-[85dvh] overflow-y-auto rounded-t-lg">
          <SheetTitle className="mb-4">Filters</SheetTitle>
          <Facets filters={filters} setFacet={setFacet} clear={clear} activeCount={activeCount} />
        </SheetContent>
      </Sheet>
    </div>
  );
}

function Facets({
  filters,
  setFacet,
  clear,
  activeCount,
}: FilterBarProps & { activeCount: number }) {
  const categories = useCategories();
  const counts = useCategoryCounts({ type: filters.type, status: filters.status, when: filters.when });
  const countFor = (id: number) => counts.data?.categories.find((c) => c.category_id === id)?.count ?? 0;

  return (
    <div className="space-y-3">
      <div className="grid grid-cols-2 gap-2 wide:flex wide:flex-wrap wide:items-end">
        <Field label="Type">
          <Select value={filters.type} onChange={(e) => setFacet("type", e.target.value as ItemType)}>
            {TYPES.map((t) => (
              <option key={t.value} value={t.value}>
                {t.label}
              </option>
            ))}
          </Select>
        </Field>
        <Field label="Status">
          <Select value={filters.status} onChange={(e) => setFacet("status", e.target.value as ItemStatus)}>
            {STATUSES.map((s) => (
              <option key={s.value} value={s.value}>
                {s.label}
              </option>
            ))}
          </Select>
        </Field>
        <Field label="When">
          <Select value={filters.when} onChange={(e) => setFacet("when", e.target.value as ItemWhen)}>
            {WHENS.map((w) => (
              <option key={w.value} value={w.value}>
                {w.label}
              </option>
            ))}
          </Select>
        </Field>
        <Field label="Sort">
          <Select value={filters.sort} onChange={(e) => setFacet("sort", e.target.value as ItemSort)}>
            {SORTS.map((s) => (
              <option key={s.value} value={s.value}>
                {s.label}
              </option>
            ))}
          </Select>
        </Field>
        {activeCount > 0 && (
          <Button variant="ghost" size="sm" className="hidden wide:inline-flex" onClick={clear}>
            <X className="size-4" /> Clear
          </Button>
        )}
      </div>

      {/* Category chips with live counts */}
      <div className="flex gap-2 overflow-x-auto pb-1">
        <Chip active={filters.cat === "all"} onClick={() => setFacet("cat", "all")}>
          All topics <Count n={counts.data?.total ?? 0} active={filters.cat === "all"} />
        </Chip>
        {categories.data?.map((c) => (
          <Chip
            key={c.id}
            active={filters.cat === c.id}
            onClick={() => setFacet("cat", filters.cat === c.id ? "all" : c.id)}
          >
            {c.name} <Count n={countFor(c.id)} active={filters.cat === c.id} />
          </Chip>
        ))}
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="flex flex-col gap-1">
      <span className="text-xs font-medium text-muted-foreground">{label}</span>
      {children}
    </label>
  );
}

function Chip({ active, onClick, children }: { active: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "inline-flex shrink-0 items-center gap-1.5 rounded-full border px-3 py-1.5 text-sm font-medium transition-colors",
        active
          ? "border-primary bg-primary text-primary-foreground"
          : "border-border bg-card text-muted-foreground hover:bg-muted",
      )}
    >
      {children}
    </button>
  );
}

function Count({ n, active }: { n: number; active: boolean }) {
  return (
    <span className={cn("text-xs", active ? "text-primary-foreground/80" : "text-muted-foreground")}>{n}</span>
  );
}

function summary(f: FeedFilters): string {
  const parts: string[] = [];
  if (f.type !== "all") parts.push(TYPES.find((t) => t.value === f.type)?.label ?? "");
  if (f.status !== "all") parts.push(STATUSES.find((s) => s.value === f.status)?.label ?? "");
  if (f.when !== "all") parts.push(WHENS.find((w) => w.value === f.when)?.label ?? "");
  return parts.length ? parts.join(" · ") : "No filters";
}
