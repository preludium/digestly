import type { QueryClient } from "@tanstack/react-query";
import {
    keepPreviousData,
    useMutation,
    useQuery,
    useQueryClient,
} from "@tanstack/react-query";
import { type FeedFilters, toItemsQuery } from "@/hooks/useFeedFilters";
import { api } from "@/lib/api";
import { sendOrQueue } from "@/lib/sync";
import type {
    CategoryCounts,
    ItemDetail,
    ItemState,
    ItemStatus,
    ItemsPage,
    ItemType,
    ItemWhen,
    SummaryResult,
} from "@/lib/types";

const ITEMS_KEY = ["items"];
const COUNTS_KEY = ["categories", "counts"];

/** Paged item list for the current filters (§10). Keeps the previous page visible while the next
 *  loads so pagination doesn't flash empty. */
export function useItems(filters: FeedFilters, enabled = true) {
    const query = toItemsQuery(filters);
    return useQuery<ItemsPage>({
        queryKey: [...ITEMS_KEY, query],
        queryFn: () => api.get<ItemsPage>(`/items${query}`),
        placeholderData: keepPreviousData,
        enabled,
    });
}

export function useItem(id: number | null) {
    return useQuery<ItemDetail>({
        queryKey: ["item", id],
        queryFn: () => api.get<ItemDetail>(`/items/${id}`),
        enabled: id != null,
    });
}

/** Chip counts reflecting the active Type/Status/When facets (not category) (§9.1). */
export function useCategoryCounts(facets: {
    type: ItemType;
    status: ItemStatus;
    when: ItemWhen;
}) {
    const p = new URLSearchParams();
    if (facets.type !== "all") p.set("type", facets.type);
    if (facets.status !== "all") p.set("status", facets.status);
    if (facets.when !== "all") p.set("when", facets.when);
    const qs = p.toString();
    return useQuery<CategoryCounts>({
        queryKey: [...COUNTS_KEY, qs],
        queryFn: () =>
            api.get<CategoryCounts>(`/categories/counts${qs ? `?${qs}` : ""}`),
    });
}

/** Total unread across all feeds - the top-bar badge (§9.0). */
export function useUnreadCount(): number {
    const counts = useCategoryCounts({
        type: "all",
        status: "unread",
        when: "all",
    });
    return counts.data?.total ?? 0;
}

/** Patch an item's state across every cached list + its detail, without a refetch (avoids the
 *  grid reshuffling under the user on a star/read click). Counts are invalidated separately. */
function patchItemState(
    qc: QueryClient,
    id: number,
    state: Partial<ItemState>,
) {
    qc.setQueriesData<ItemsPage>({ queryKey: ITEMS_KEY }, (old) =>
        old
            ? {
                  ...old,
                  items: old.items.map((it) =>
                      it.id === id ? { ...it, ...state } : it,
                  ),
              }
            : old,
    );
    qc.setQueryData<ItemDetail>(["item", id], (old) =>
        old ? { ...old, ...state } : old,
    );
}

// Read/star mutations are offline-capable (S3): the cache is patched immediately, then the write is
// sent - or queued in the outbox and replayed on reconnect if we're offline. Counts refresh when
// online (they need the network; offline they stay optimistic until the next successful fetch).
export function useToggleRead() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: async ({ id, value }: { id: number; value: boolean }) => {
            patchItemState(qc, id, { is_read: value });
            return sendOrQueue("read", id, value);
        },
        onSuccess: () => qc.invalidateQueries({ queryKey: COUNTS_KEY }),
    });
}

export function useToggleStar() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: async ({ id, value }: { id: number; value: boolean }) => {
            patchItemState(qc, id, { is_starred: value });
            return sendOrQueue("star", id, value);
        },
        onSuccess: () => qc.invalidateQueries({ queryKey: COUNTS_KEY }),
    });
}

/** Summarize (or regenerate) an item via the active AI provider (§6, §6a). The result lands in the
 *  shared cache; we patch it into the open detail immediately, then refetch to pick up any newly
 *  fetched transcript + reading time. */
export function useSummarize() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: ({ id, force }: { id: number; force?: boolean }) =>
            api.post<SummaryResult>(
                `/items/${id}/summarize${force ? "?force=1" : ""}`,
            ),
        onSuccess: (res, { id }) => {
            qc.setQueryData<ItemDetail>(["item", id], (old) =>
                old
                    ? {
                          ...old,
                          summary: res.summary,
                          summary_kind: res.summary_kind,
                          has_summary: true,
                      }
                    : old,
            );
            qc.invalidateQueries({ queryKey: ["item", id] });
            qc.invalidateQueries({ queryKey: ITEMS_KEY });
        },
    });
}
