import {
    ChevronDown,
    MoreVertical,
    Pencil,
    Plus,
    RefreshCw,
    Rss,
    Trash2,
} from "lucide-react";
import { useMemo, useState } from "react";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { EmptyState } from "@/components/common/EmptyState";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { NameDialog } from "@/components/common/NameDialog";
import { FeedEditModal } from "@/components/feeds/FeedEditModal";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
    Collapsible,
    CollapsibleContent,
    CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import {
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
    useCategories,
    useCreateCategory,
    useDeleteCategory,
    useUpdateCategory,
} from "@/hooks/useCategories";
import { useFeeds, useRefreshFeed, useUnsubscribe } from "@/hooks/useFeeds";
import { kindIcon, kindLabel } from "@/lib/format";
import type { Category, Feed } from "@/lib/types";
import { cn } from "@/lib/utils";
import { toast } from "@/stores/toast";
import { useUiStore } from "@/stores/ui";
import { sortCategoriesOtherLast } from "./manage.helpers";

/** Manage categories & feeds - the structural hub (prompt.md §9.5). */
export function Manage() {
    const feeds = useFeeds();
    const categories = useCategories();
    const openAddFeed = useUiStore((s) => s.setAddFeedOpen);

    const [categoryFilter, setCategoryFilter] = useState<number | "all">("all");
    const [search, setSearch] = useState("");
    const [editing, setEditing] = useState<Feed | null>(null);
    const [creating, setCreating] = useState(false);

    const createCategory = useCreateCategory();

    const grouped = useMemo(
        () => groupByCategory(categories.data ?? [], feeds.data ?? []),
        [categories.data, feeds.data],
    );

    const visible = grouped
        .filter(
            (g) => categoryFilter === "all" || g.category.id === categoryFilter,
        )
        .map((g) => ({
            ...g,
            feeds: g.feeds.filter((f) =>
                f.title.toLowerCase().includes(search.trim().toLowerCase()),
            ),
        }));

    const handleCreateCategory = (name: string) => {
        createCategory.mutate(name, {
            onSuccess: () => toast("Category created", "success"),
            onError: (e) =>
                toast(
                    e instanceof Error ? e.message : "Could not create",
                    "error",
                ),
        });
    };

    const isLoading = feeds.isLoading || categories.isLoading;
    const isError = feeds.isError || categories.isError;
    const noFeeds = (feeds.data?.length ?? 0) === 0;

    return (
        <div className="space-y-6">
            <div className="flex flex-wrap items-center justify-between gap-3">
                <h1 className="font-display text-2xl font-semibold tracking-tight">
                    Manage
                </h1>
                <div className="flex gap-2">
                    <Button onClick={() => openAddFeed(true)}>
                        <Plus className="size-4" /> Add feed
                    </Button>
                    <Button variant="outline" onClick={() => setCreating(true)}>
                        <Plus className="size-4" /> New category
                    </Button>
                </div>
            </div>

            {!noFeeds && (
                <div className="flex flex-wrap gap-3">
                    <Select
                        value={String(categoryFilter)}
                        onValueChange={(v) =>
                            setCategoryFilter(v === "all" ? "all" : Number(v))
                        }
                    >
                        <SelectTrigger className="w-full sm:max-w-xs">
                            <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                            <SelectItem value="all">All categories</SelectItem>
                            {categories.data?.map((c) => (
                                <SelectItem key={c.id} value={String(c.id)}>
                                    {c.name} ({c.feed_count})
                                </SelectItem>
                            ))}
                        </SelectContent>
                    </Select>
                    <Input
                        className="w-full sm:max-w-xs"
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
                visible.map((g) => (
                    <CategoryCard
                        key={g.category.id}
                        category={g.category}
                        feeds={g.feeds}
                        onEdit={setEditing}
                    />
                ))}

            <FeedEditModal feed={editing} onClose={() => setEditing(null)} />

            <NameDialog
                open={creating}
                onOpenChange={setCreating}
                title="New category"
                label="Name"
                submitLabel="Create"
                onSubmit={handleCreateCategory}
            />
        </div>
    );
}

function groupByCategory(categories: Category[], feeds: Feed[]) {
    return sortCategoriesOtherLast(categories).map((category) => ({
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

    const [open, setOpen] = useState(true);
    const [renaming, setRenaming] = useState(false);
    const [deleting, setDeleting] = useState(false);

    const handleRename = (name: string) => {
        rename.mutate(
            { id: category.id, name },
            {
                onSuccess: () => toast("Category renamed", "success"),
                onError: (e) =>
                    toast(
                        e instanceof Error ? e.message : "Could not rename",
                        "error",
                    ),
            },
        );
    };

    const handleDelete = () => {
        del.mutate(category.id, {
            onSuccess: () =>
                toast("Category deleted; feeds moved to Other", "success"),
            onError: (e) =>
                toast(
                    e instanceof Error ? e.message : "Could not delete",
                    "error",
                ),
        });
    };

    return (
        <Collapsible open={open} onOpenChange={setOpen}>
            <Card>
                <div
                    className={cn(
                        "flex flex-row items-center justify-between gap-2 p-6",
                        open ? "pb-0" : "pb-6",
                    )}
                >
                    <CollapsibleTrigger className="flex items-center gap-2 rounded-md hover:bg-muted/50 -ml-1 px-1 py-0.5">
                        <ChevronDown
                            className={cn(
                                "size-4 transition-transform duration-200",
                                open && "rotate-180",
                            )}
                        />
                        <div className="flex items-baseline gap-2">
                            <h2 className="text-lg font-semibold">
                                {category.name}
                            </h2>
                            <span className="text-sm text-muted-foreground">
                                {feeds.length}
                            </span>
                        </div>
                    </CollapsibleTrigger>
                    <div className="flex gap-1">
                        <Button
                            variant="ghost"
                            size="sm"
                            aria-label={`Add feed to ${category.name}`}
                            onClick={() => openAddFeed(true, category.id)}
                        >
                            <Plus className="size-4" />
                        </Button>
                        {category.deletable && (
                            <Button
                                variant="ghost"
                                size="sm"
                                onClick={() => setRenaming(true)}
                                aria-label="Rename"
                            >
                                <Pencil className="size-4" />
                            </Button>
                        )}
                        {category.deletable && (
                            <Button
                                variant="ghost"
                                size="sm"
                                onClick={() => setDeleting(true)}
                                aria-label="Delete category"
                            >
                                <Trash2 className="size-4" />
                            </Button>
                        )}
                    </div>
                </div>
                <CollapsibleContent>
                    <CardContent>
                        {feeds.length === 0 ? (
                            <p className="text-sm text-muted-foreground">
                                No feeds in this category.
                            </p>
                        ) : (
                            <ul className="divide-y divide-border">
                                {feeds.map((f) => (
                                    <FeedRow
                                        key={f.id}
                                        feed={f}
                                        onEdit={onEdit}
                                    />
                                ))}
                            </ul>
                        )}
                    </CardContent>
                </CollapsibleContent>
            </Card>
            <NameDialog
                open={renaming}
                onOpenChange={setRenaming}
                title="Rename category"
                label="Name"
                initialValue={category.name}
                submitLabel="Rename"
                onSubmit={handleRename}
            />
            <ConfirmDialog
                open={deleting}
                onOpenChange={setDeleting}
                title={`Delete "${category.name}"?`}
                description="Its feeds move to Other."
                confirmLabel="Delete"
                destructive
                onConfirm={handleDelete}
            />
        </Collapsible>
    );
}

function FeedRow({ feed, onEdit }: { feed: Feed; onEdit: (f: Feed) => void }) {
    const refresh = useRefreshFeed();
    const unsubscribe = useUnsubscribe();

    const [unsubscribing, setUnsubscribing] = useState(false);

    const metaParts = [
        kindLabel(feed.kind),
        feed.content_type === "video" ? "🎬 video" : "📖 reading",
        `${feed.item_count} items`,
    ];

    return (
        <li className="flex items-center gap-3 py-2.5">
            <span className="text-lg" title={kindLabel(feed.kind)}>
                {kindIcon(feed.kind)}
            </span>
            <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                    <span className="truncate text-sm font-medium">
                        {feed.title}
                    </span>
                    {feed.disabled && <Badge variant="secondary">paused</Badge>}
                    {feed.feed_disabled && (
                        <Badge variant="destructive">disabled</Badge>
                    )}
                    {feed.kind === "reddit" && (
                        <Badge variant="outline">min ▲ {feed.min_score}</Badge>
                    )}
                </div>
                <div className="mt-1 flex flex-wrap items-center gap-1.5">
                    {metaParts.map((part) => (
                        <span
                            key={part}
                            className="inline-flex items-center whitespace-nowrap rounded-md bg-muted px-2 py-0.5 text-[11px] font-semibold text-muted-foreground"
                        >
                            {part}
                        </span>
                    ))}
                </div>
            </div>
            <div className="flex shrink-0">
                <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                        <Button
                            variant="ghost"
                            size="icon"
                            aria-label="Actions"
                        >
                            <MoreVertical className="size-4" />
                        </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                        <DropdownMenuItem
                            disabled={refresh.isPending}
                            onClick={() =>
                                refresh.mutate(feed.id, {
                                    onSuccess: () => toast("Refreshing…"),
                                })
                            }
                        >
                            <RefreshCw className="size-4" /> Refresh
                        </DropdownMenuItem>
                        <DropdownMenuItem onClick={() => onEdit(feed)}>
                            <Pencil className="size-4" /> Edit
                        </DropdownMenuItem>
                        <DropdownMenuItem
                            className="text-destructive focus:bg-destructive/10 focus:text-destructive"
                            onClick={() => setUnsubscribing(true)}
                        >
                            <Trash2 className="size-4" /> Unsubscribe
                        </DropdownMenuItem>
                    </DropdownMenuContent>
                </DropdownMenu>
            </div>
            <ConfirmDialog
                open={unsubscribing}
                onOpenChange={setUnsubscribing}
                title={`Unsubscribe from "${feed.title}"?`}
                confirmLabel="Unsubscribe"
                destructive
                onConfirm={() => {
                    unsubscribe.mutate(feed.id, {
                        onSuccess: () => toast("Unsubscribed", "success"),
                    });
                }}
            />
        </li>
    );
}
