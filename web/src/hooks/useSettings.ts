import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api";
import type { IngestionSettings, UserSettings } from "@/lib/types";

// Per-user preferences (§9.7 General) + admin ingestion settings (§8).

const SETTINGS_KEY = ["settings"];
const INGESTION_KEY = ["admin", "ingestion"];

export function useSettings() {
  return useQuery<UserSettings>({
    queryKey: SETTINGS_KEY,
    queryFn: () => api.get<UserSettings>("/settings"),
    staleTime: 30_000,
  });
}

export function useUpdateSettings() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (body: Partial<UserSettings>) => api.put<UserSettings>("/settings", body),
    onSuccess: (data) => {
      qc.setQueryData(SETTINGS_KEY, data);
      // Preferences influence the item list (sort/page size/view) and counts.
      qc.invalidateQueries({ queryKey: ["items"] });
    },
  });
}

/** Onboarding opt-in: subscribe to the §3 starter feeds. */
export function useSubscribeStarterFeeds() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => api.post<{ added: number }>("/onboarding/starter-feeds"),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["feeds"] });
      qc.invalidateQueries({ queryKey: ["categories"] });
    },
  });
}

export function useIngestionSettings() {
  return useQuery<IngestionSettings>({
    queryKey: INGESTION_KEY,
    queryFn: () => api.get<IngestionSettings>("/admin/ingestion"),
  });
}

export function useUpdateIngestionSettings() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (body: IngestionSettings) => api.put<IngestionSettings>("/admin/ingestion", body),
    onSuccess: (data) => qc.setQueryData(INGESTION_KEY, data),
  });
}

/** Admin-only: apply the saved retention policy immediately instead of waiting for the periodic
 *  6h maintenance task. Uses whatever retention settings are currently saved (§8). */
export function usePurgeRetention() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => api.post<{ removed: number }>("/admin/retention/purge"),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["items"] });
      qc.invalidateQueries({ queryKey: ["feeds"] });
    },
  });
}
