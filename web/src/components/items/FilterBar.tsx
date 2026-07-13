import { ArrowDownWideNarrow, SlidersHorizontal, X } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import {
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
} from "@/components/ui/select";
import { Sheet, SheetContent, SheetTitle } from "@/components/ui/sheet";
import { useCategories } from "@/hooks/useCategories";
import { activeFilterCount, type FeedFilters } from "@/hooks/useFeedFilters";
import { useCategoryCounts } from "@/hooks/useItems";
import { topicBadgeClass } from "@/lib/topicColor";
import type { ItemSort, ItemStatus, ItemType, ItemWhen } from "@/lib/types";
import { cn } from "@/lib/utils";

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
    { value: "24h", label: "Last 24 hours" },
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
    setFacet: <K extends keyof FeedFilters>(
        key: K,
        value: FeedFilters[K],
    ) => void;
    clear: () => void;
    resultCount: number;
}

/** Three-tier filter bar (mockup-alignment §3): topic chips, then a bordered "refine" segment
 *  (Type/Status/When as one combined pill) + a separate sort pill + live result count. Inline on
 *  desktop (≥820px / `wide:`), collapses the refine segment behind a Sheet on mobile. */
export function FilterBar({
    filters,
    setFacet,
    clear,
    resultCount,
}: FilterBarProps) {
    const [open, setOpen] = useState(false);
    const activeCount = activeFilterCount(filters);

    return (
        <div className="space-y-4">
            <TopicChips filters={filters} setFacet={setFacet} />

            <div className="flex flex-wrap items-center justify-between gap-3 border-t border-border pt-4">
                <div className="hidden items-center gap-2 wide:flex">
                    <RefinePill filters={filters} setFacet={setFacet} />
                    {activeCount > 0 && (
                        <Button variant="ghost" size="sm" onClick={clear}>
                            <X className="size-4" /> Clear
                        </Button>
                    )}
                </div>

                <div className="wide:hidden">
                    <Button
                        variant="outline"
                        size="sm"
                        onClick={() => setOpen(true)}
                    >
                        <SlidersHorizontal className="size-4" />
                        Filters
                    </Button>
                </div>

                <div className="ml-auto flex items-center gap-3">
                    <p className="text-sm text-muted-foreground">
                        <span className="font-semibold text-foreground">
                            {resultCount}
                        </span>{" "}
                        articles
                    </p>
                    <SortPill filters={filters} setFacet={setFacet} />
                </div>
            </div>

            <Sheet open={open} onOpenChange={setOpen}>
                <SheetContent
                    side="bottom"
                    className="max-h-[85dvh] overflow-y-auto rounded-t-lg"
                >
                    <SheetTitle className="mb-4">Filters</SheetTitle>
                    <RefinePill filters={filters} setFacet={setFacet} mobile />
                    {activeCount > 0 && (
                        <Button
                            variant="ghost"
                            size="sm"
                            className="mt-3"
                            onClick={() => {
                                clear();
                                setOpen(false);
                            }}
                        >
                            <X className="size-4" /> Clear all
                        </Button>
                    )}
                </SheetContent>
            </Sheet>
        </div>
    );
}

/** "All topics" chip separated by a divider from the scrollable topic chip list, which fades at
 *  its scroll edges (mockup: `mask-image` gradient). */
function TopicChips({
    filters,
    setFacet,
}: Pick<FilterBarProps, "filters" | "setFacet">) {
    const categories = useCategories();
    const counts = useCategoryCounts({
        type: filters.type,
        status: filters.status,
        when: filters.when,
    });
    const countFor = (id: number) =>
        counts.data?.categories.find((c) => c.category_id === id)?.count ?? 0;
    const sortedCategories = [...(categories.data ?? [])].sort(
        (a, b) => countFor(b.id) - countFor(a.id),
    );

    return (
        <div className="flex items-center gap-2">
            <div className="shrink-0">
                <Chip
                    active={filters.cat === "all"}
                    onClick={() => setFacet("cat", "all")}
                >
                    All topics{" "}
                    <Count
                        n={counts.data?.total ?? 0}
                        active={filters.cat === "all"}
                    />
                </Chip>
            </div>
            <span className="h-6 w-px shrink-0 bg-border" />
            <div
                className="flex gap-2 overflow-x-auto px-1 pb-1"
                style={{
                    maskImage:
                        "linear-gradient(to right, transparent 0, #000 12px, #000 calc(100% - 12px), transparent 100%)",
                    WebkitMaskImage:
                        "linear-gradient(to right, transparent 0, #000 12px, #000 calc(100% - 12px), transparent 100%)",
                }}
            >
                {sortedCategories.map((c) => (
                    <Chip
                        key={c.id}
                        active={filters.cat === c.id}
                        onClick={() =>
                            setFacet("cat", filters.cat === c.id ? "all" : c.id)
                        }
                    >
                        {c.name}{" "}
                        <Count
                            n={countFor(c.id)}
                            active={filters.cat === c.id}
                            colorClass={topicBadgeClass(c.name)}
                        />
                    </Chip>
                ))}
            </div>
        </div>
    );
}

/** Type/Status/When combined into one bordered segmented pill (desktop), or a stacked block
 *  inside the mobile Sheet. */
function RefinePill({
    filters,
    setFacet,
    mobile = false,
}: Pick<FilterBarProps, "filters" | "setFacet"> & { mobile?: boolean }) {
    const triggerClass = mobile
        ? "h-11 flex-1 justify-end gap-1 rounded-none border-0 bg-transparent px-1 text-base font-semibold shadow-none focus-visible:ring-0 focus-visible:ring-offset-0"
        : "h-8 w-auto max-w-[9.5rem] gap-1 rounded-none border-0 bg-transparent px-1 text-sm font-semibold shadow-none focus-visible:ring-0 focus-visible:ring-offset-0";

    return (
        <div
            className={cn(
                "flex w-fit items-center divide-x divide-border rounded-lg border border-border bg-card",
                mobile && "w-full flex-col items-stretch divide-x-0 divide-y",
            )}
        >
            <FacetField label="Type" mobile={mobile}>
                <Select
                    value={filters.type}
                    onValueChange={(v) => setFacet("type", v as ItemType)}
                >
                    <SelectTrigger className={triggerClass}>
                        <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                        {TYPES.map((t) => (
                            <SelectItem key={t.value} value={t.value}>
                                {t.label}
                            </SelectItem>
                        ))}
                    </SelectContent>
                </Select>
            </FacetField>
            <FacetField label="Status" mobile={mobile}>
                <Select
                    value={filters.status}
                    onValueChange={(v) => setFacet("status", v as ItemStatus)}
                >
                    <SelectTrigger className={triggerClass}>
                        <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                        {STATUSES.map((s) => (
                            <SelectItem key={s.value} value={s.value}>
                                {s.label}
                            </SelectItem>
                        ))}
                    </SelectContent>
                </Select>
            </FacetField>
            <FacetField label="When" mobile={mobile}>
                <Select
                    value={filters.when}
                    onValueChange={(v) => setFacet("when", v as ItemWhen)}
                >
                    <SelectTrigger className={triggerClass}>
                        <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                        {WHENS.map((w) => (
                            <SelectItem key={w.value} value={w.value}>
                                {w.label}
                            </SelectItem>
                        ))}
                    </SelectContent>
                </Select>
            </FacetField>
        </div>
    );
}

function FacetField({
    label,
    children,
    mobile = false,
}: {
    label: string;
    children: React.ReactNode;
    mobile?: boolean;
}) {
    return (
        <label
            className={cn(
                "flex items-center gap-1.5 px-2 py-1",
                mobile && "gap-3 px-3",
            )}
        >
            <span
                className={cn(
                    "shrink-0 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground",
                    mobile && "text-sm",
                )}
            >
                {label}
            </span>
            {children}
        </label>
    );
}

/** Sort as its own rounded pill (muted bg, sort-arrows icon) - visually distinct from Refine. */
function SortPill({
    filters,
    setFacet,
}: Pick<FilterBarProps, "filters" | "setFacet">) {
    return (
        <label className="inline-flex h-8 shrink-0 items-center gap-1.5 rounded-full bg-muted pl-3 pr-1">
            <ArrowDownWideNarrow className="size-3.5 shrink-0 text-muted-foreground" />
            <Select
                value={filters.sort}
                onValueChange={(v) => setFacet("sort", v as ItemSort)}
            >
                <SelectTrigger className="h-7 w-auto gap-1 rounded-full border-0 bg-transparent px-1 text-sm font-semibold shadow-none focus-visible:ring-0 focus-visible:ring-offset-0">
                    <SelectValue />
                </SelectTrigger>
                <SelectContent>
                    {SORTS.map((s) => (
                        <SelectItem key={s.value} value={s.value}>
                            {s.label}
                        </SelectItem>
                    ))}
                </SelectContent>
            </Select>
        </label>
    );
}

function Chip({
    active,
    onClick,
    children,
}: {
    active: boolean;
    onClick: () => void;
    children: React.ReactNode;
}) {
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

function Count({
    n,
    active,
    colorClass,
}: {
    n: number;
    active: boolean;
    colorClass?: string;
}) {
    return (
        <span
            className={cn(
                "inline-flex min-w-[1.25rem] items-center justify-center rounded-full px-1.5 py-0.5 text-xs font-semibold leading-none",
                active
                    ? "bg-primary-foreground/20 text-primary-foreground"
                    : colorClass
                      ? colorClass
                      : "bg-muted text-muted-foreground",
            )}
        >
            {n}
        </span>
    );
}
