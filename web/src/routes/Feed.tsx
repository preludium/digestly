import { Rss, Search as SearchIcon } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useLocation, useNavigate, useSearchParams } from "react-router-dom";
import { EmptyState } from "@/components/common/EmptyState";
import { IngestButton } from "@/components/feeds/IngestButton";
import { FilterBar } from "@/components/items/FilterBar";
import { ItemGrid } from "@/components/items/ItemGrid";
import { ItemPreview } from "@/components/items/ItemPreview";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useFeedFilters } from "@/hooks/useFeedFilters";
import { useFeeds } from "@/hooks/useFeeds";
import { useIngestNow } from "@/hooks/useIngest";
import { useItems, useToggleRead, useToggleStar } from "@/hooks/useItems";
import { externalHref } from "@/lib/externalLink";
import type { Item } from "@/lib/types";
import { useUiStore } from "@/stores/ui";

/** The core feed screen - responsive card grid with unified search + filters. Search used to be
 *  a separate /search route; it now lives inline here (mockup-alignment §1) so there's a single
 *  browsing surface. */
export function Feed() {
    const { filters, setFacet, setPage, clear } = useFeedFilters(true);
    const items = useItems(filters);
    const feeds = useFeeds();
    const ingest = useIngestNow();
    const toggleRead = useToggleRead();
    const toggleStar = useToggleStar();
    const openAddFeed = useUiStore((s) => s.setAddFeedOpen);

    const [params, setParams] = useSearchParams();
    const location = useLocation();
    const navigate = useNavigate();

    // The open item lives in the URL (`?item=<id>`), not in component state, so that opening one
    // pushes a history entry: Back (incl. Android's) then closes the preview instead of leaving
    // the feed. It also makes an open article shareable/refreshable.
    const previewId = Number(params.get("item")) || null;
    // The clicked card, so the sheet can render its header instantly instead of waiting on detail.
    // The cached list row wins over it: read/star mutations patch the cache, the seed is frozen.
    const [seed, setSeed] = useState<Item | null>(null);
    const previewItem =
        previewId == null
            ? null
            : (items.data?.items.find((i) => i.id === previewId) ??
              (seed?.id === previewId ? seed : null));

    const [text, setText] = useState(filters.q);
    const searchRef = useRef<HTMLInputElement>(null);
    const closing = useRef(false);

    const openPreview = useCallback(
        (item: Item) => {
            setSeed(item);
            const p = new URLSearchParams(params);
            p.set("item", String(item.id));
            setParams(p, { state: { previewPushed: true } });
        },
        [params, setParams],
    );

    // navigate(-1) is async: until popstate lands the sheet is still mounted, so a double-tap on
    // the overlay would pop twice and take the user off the feed.
    // biome-ignore lint/correctness/useExhaustiveDependencies: existing baseline
    useEffect(() => {
        closing.current = false;
    }, [previewId]);

    const closePreview = useCallback(() => {
        if (closing.current) return;
        closing.current = true;
        // Pop the entry we pushed, so closing via Back, Escape or the overlay all land in the same
        // place and leave no phantom entry to back through twice.
        if (
            (location.state as { previewPushed?: boolean } | null)
                ?.previewPushed
        ) {
            navigate(-1);
            return;
        }
        // Deep link or reload: there is nothing of ours to pop, so drop the param in place rather
        // than sending the user back out of the app.
        const p = new URLSearchParams(params);
        p.delete("item");
        setParams(p, { replace: true });
    }, [location.state, navigate, params, setParams]);

    // Debounce the search box into the URL (source of truth for filters).
    // Held while the preview is open: a filter write pushes a history entry WITHOUT the
    // previewPushed marker, which would strand the entry we pushed (see closePreview). It fires
    // once the preview closes, since previewId is a dependency.
    useEffect(() => {
        if (previewId != null || text === filters.q) return;
        const id = setTimeout(() => setFacet("q", text), 300);
        return () => clearTimeout(id);
    }, [text, filters.q, previewId, setFacet]);

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
            // The sheet is modal but this listener is on window, and its content is a div - so the
            // form-control guard above does not catch it. Grid keys must not fire behind the
            // preview: paging it is meaningless, and any filter write pushes a history entry
            // without the previewPushed marker, stranding the entry we pushed (see closePreview).
            const previewOpen = previewId != null;
            switch (e.key) {
                case "n":
                    if (
                        !previewOpen &&
                        items.data &&
                        filters.page < items.data.total_pages
                    )
                        setPage(filters.page + 1);
                    break;
                case "p":
                    if (!previewOpen && filters.page > 1)
                        setPage(filters.page - 1);
                    break;
                case "r":
                    e.preventDefault();
                    ingest.mutate();
                    break;
                case "/":
                    if (previewOpen) break;
                    e.preventDefault();
                    searchRef.current?.focus();
                    break;
                case "o":
                    if (previewItem?.url)
                        window.open(
                            externalHref(previewItem.url, previewItem.kind),
                            "_blank",
                            "noopener",
                        );
                    break;
                case "m":
                    if (previewItem)
                        toggleRead.mutate({
                            id: previewItem.id,
                            value: !previewItem.is_read,
                        });
                    break;
                case "s":
                    if (previewItem)
                        toggleStar.mutate({
                            id: previewItem.id,
                            value: !previewItem.is_starred,
                        });
                    break;
            }
        };
        window.addEventListener("keydown", onKey);
        return () => window.removeEventListener("keydown", onKey);
    }, [
        items.data,
        filters.page,
        previewId,
        previewItem,
        setPage,
        ingest,
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
                <IngestButton />
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
                onOpen={openPreview}
                onPage={setPage}
                emptyTitle={
                    filters.q.trim()
                        ? `No results for "${filters.q}"`
                        : "Nothing matches these filters 🎉"
                }
                emptyDescription="Try clearing a filter or widening the time range."
            />
            <ItemPreview
                itemId={previewId}
                seed={previewItem}
                onClose={closePreview}
            />
        </div>
    );
}
