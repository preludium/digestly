import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api";
import type { DigestConfig, DigestDetailData, DigestListItem, DigestRunSummary, PutDigestConfig } from "@/lib/types";

// Digest history is per-user; config + run are admin-only (server-enforced, prompt.md §7, §10).

const LIST_KEY = ["digests"];
const CONFIG_KEY = ["digest", "config"];

export function useDigests() {
  return useQuery<DigestListItem[]>({
    queryKey: LIST_KEY,
    queryFn: () => api.get<DigestListItem[]>("/digest"),
  });
}

export function useDigest(id: number) {
  return useQuery<DigestDetailData>({
    queryKey: ["digest", id],
    queryFn: () => api.get<DigestDetailData>(`/digest/${id}`),
  });
}

export function useDigestConfig() {
  return useQuery<DigestConfig>({
    queryKey: CONFIG_KEY,
    queryFn: () => api.get<DigestConfig>("/digest/config"),
  });
}

export function useUpdateDigestConfig() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (body: PutDigestConfig) => api.put<DigestConfig>("/digest/config", body),
    onSuccess: (data) => qc.setQueryData(CONFIG_KEY, data),
  });
}

/** Admin-only: run the digest for all users now. `lookbackDays`, if given, overrides the
 *  configured window for this run only (e.g. a one-off "last month" catch-up run). */
export function useRunDigest() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (lookbackDays?: number) =>
      api.post<DigestRunSummary>("/digest/run", lookbackDays ? { lookback_days: lookbackDays } : undefined),
    onSuccess: () => qc.invalidateQueries({ queryKey: LIST_KEY }),
  });
}
