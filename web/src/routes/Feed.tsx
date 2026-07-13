import { RefreshCw, Rss, Search as SearchIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { EmptyState } from "@/components/common/EmptyState";
import { FilterBar } from "@/components/items/FilterBar";
import { ItemGrid } from "@/components/items/ItemGrid";
import { ItemPreview } from "@/components/items/ItemPreview";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useFeedFilters } from "@/hooks/useFeedFilters";
import { useFeeds, useRefreshAll } from "@/hooks/useFeeds";
import { useItems, useToggleRead, useToggleStar } from "@/hooks/useItems";
import type { Item } from "@/lib/types";
import { cn } from "@/lib/utils";
import { toast } from "@/stores/toast";
import { useUiStore } from "@/stores/ui";

/** The core feed screen - responsive card grid with unified search + filters. Search used to be
 *  a separate /search route; it now lives inline here (mockup-alignment §1) so there's a single
 *  browsing surface. */
export function Feed() {
    const { filters, setFacet, setPage, clear } = useFeedFilters(true);
    const items = useItems(filters);
    const feeds = useFeeds();
    const refreshAll = useRefreshAll();
    const toggleRead = useToggleRead();
    const toggleStar = useToggleStar();
    const openAddFeed = useUiStore((s) => s.setAddFeedOpen);

    const [preview, setPreview] = useState<Item | null>(null);
    const [text, setText] = useState(filters.q);
    const searchRef = useRef<HTMLInputElement>(null);

    // Debounce the search box into the URL (source of truth for filters).
    useEffect(() => {
        if (text === filters.q) return;
        const id = setTimeout(() => setFacet("q", text), 300);
        return () => clearTimeout(id);
    }, [text, filters.q, setFacet]);

    // Keyboard shortcuts (§9.1). Ignored while typing in a form control.
    useEffect(() => {
        const onKey = (e: KeyboardEvent) => {
            const t = e.target as HTMLElement | null;
            if (
                t &&
                (t.tagName === "INPUT" ||
                    t.tagName === "TEXTAREA" ||
                    t.tagName === "SELECT" ||
                    t.isContentEditable)
            )
                return;
            if (e.metaKey || e.ctrlKey || e.altKey) return;
            switch (e.key) {
                case "n":
                    if (items.data && filters.page < items.data.total_pages)
                        setPage(filters.page + 1);
                    break;
                case "p":
                    if (filters.page > 1) setPage(filters.page - 1);
                    break;
                case "r":
                    e.preventDefault();
                    refreshAll.mutate(undefined, {
                        onSuccess: () => toast("Refreshing feeds…"),
                    });
                    break;
                case "/":
                    e.preventDefault();
                    searchRef.current?.focus();
                    break;
                case "o":
                    if (preview?.url)
                        window.open(preview.url, "_blank", "noopener");
                    break;
                case "m":
                    if (preview)
                        toggleRead.mutate({
                            id: preview.id,
                            value: !preview.is_read,
                        });
                    break;
                case "s":
                    if (preview)
                        toggleStar.mutate({
                            id: preview.id,
                            value: !preview.is_starred,
                        });
                    break;
            }
        };
        window.addEventListener("keydown", onKey);
        return () => window.removeEventListener("keydown", onKey);
    }, [
        items.data,
        filters.page,
        preview,
        setPage,
        refreshAll,
        toggleRead,
        toggleStar,
    ]);

    // First-run: no subscriptions yet.
    if (feeds.data && feeds.data.length === 0) {
        return (
            <EmptyState
                icon={<Rss className="size-8" />}
                title="No feeds yet"
                description="Add your first feed to start building your digestly."
                action={
                    <Button onClick={() => openAddFeed(true)}>
                        Add your first feed
                    </Button>
                }
            />
        );
    }

    return (
        <div className="space-y-4">
            <div className="flex flex-wrap items-center justify-between gap-3">
                <h1 className="font-display text-2xl font-semibold tracking-tight">
                    Your feed
                </h1>
                <Button
                    variant="ghost"
                    size="icon"
                    aria-label="Refresh all feeds"
                    disabled={refreshAll.isPending}
                    onClick={() =>
                        refreshAll.mutate(undefined, {
                            onSuccess: () => toast("Refreshing feeds…"),
                        })
                    }
                >
                    <RefreshCw
                        className={cn(
                            "size-5",
                            refreshAll.isPending && "animate-spin",
                        )}
                    />
                </Button>
            </div>

            <div className="relative">
                <SearchIcon className="pointer-events-none absolute left-3.5 top-1/2 size-4.5 -translate-y-1/2 text-muted-foreground" />
                <Input
                    ref={searchRef}
                    aria-label="Search articles"
                    placeholder="Search articles"
                    className="h-11.5 rounded-xl border-border bg-card pl-11 text-[15px] shadow-sm"
                    value={text}
                    onChange={(e) => setText(e.target.value)}
                />
            </div>

            <FilterBar
                filters={filters}
                setFacet={setFacet}
                clear={clear}
                resultCount={items.data?.total_count ?? 0}
            />
            <ItemGrid
                data={items.data}
                isLoading={items.isLoading}
                isError={items.isError}
                error={items.error}
                filters={filters}
                onOpen={setPreview}
                onPage={setPage}
                emptyTitle={
                    filters.q.trim()
                        ? `No results for "${filters.q}"`
                        : "Nothing matches these filters 🎉"
                }
                emptyDescription="Try clearing a filter or widening the time range."
            />
            <ItemPreview item={preview} onClose={() => setPreview(null)} />
        </div>
    );
}
