import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api";
import type { DiscoverCandidate, Feed, FeedHealth, SubscribeInput } from "@/lib/types";

const FEEDS_KEY = ["feeds"];
const HEALTH_KEY = ["feeds", "health"];

/** Invalidate everything that reflects subscription/feed changes. */
function useInvalidateFeeds() {
  const qc = useQueryClient();
  return () => {
    qc.invalidateQueries({ queryKey: FEEDS_KEY });
    qc.invalidateQueries({ queryKey: HEALTH_KEY });
    qc.invalidateQueries({ queryKey: ["categories"] });
  };
}

export function useFeeds() {
  return useQuery<Feed[]>({ queryKey: FEEDS_KEY, queryFn: () => api.get<Feed[]>("/feeds") });
}

export function useFeedHealth() {
  return useQuery<FeedHealth[]>({
    queryKey: HEALTH_KEY,
    queryFn: () => api.get<FeedHealth[]>("/feeds/health"),
    // Feeds poll in the background; keep health reasonably fresh.
    refetchInterval: 30_000,
  });
}

/** Count of feeds needing attention — drives the nav "red dot" (§9.0). */
export function useUnhealthyCount(): number {
  const health = useFeedHealth();
  return (health.data ?? []).filter((f) => f.status !== "ok").length;
}

export function useDiscover() {
  return useMutation({
    mutationFn: (input: string) => api.post<DiscoverCandidate[]>("/feeds/discover", { input }),
  });
}

export function useSubscribe() {
  const invalidate = useInvalidateFeeds();
  return useMutation({
    mutationFn: (body: SubscribeInput) => api.post<Feed>("/feeds", body),
    onSuccess: invalidate,
  });
}

export function useUpdateFeed() {
  const invalidate = useInvalidateFeeds();
  return useMutation({
    mutationFn: ({ id, ...body }: { id: number } & Partial<SubscribeInput> & { disabled?: boolean }) =>
      api.patch<Feed>(`/feeds/${id}`, body),
    onSuccess: invalidate,
  });
}

export function useUnsubscribe() {
  const invalidate = useInvalidateFeeds();
  return useMutation({
    mutationFn: (id: number) => api.del<{ ok: boolean }>(`/feeds/${id}`),
    onSuccess: invalidate,
  });
}

export function useRefreshFeed() {
  const invalidate = useInvalidateFeeds();
  return useMutation({
    mutationFn: (id: number) => api.post<{ ok: boolean }>(`/feeds/${id}/refresh`),
    onSuccess: invalidate,
  });
}

/** Top-bar "refresh all" (§9.0): re-poll every subscription in one request, then refetch items. */
export function useRefreshAll() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => api.post<{ ok: boolean; feeds: number }>("/feeds/refresh-all"),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["items"] });
      qc.invalidateQueries({ queryKey: HEALTH_KEY });
    },
  });
}
