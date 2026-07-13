import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api";
import type {
    AiPreset,
    AiProvider,
    AiSettings,
    NewAiProvider,
    TestResult,
} from "@/lib/types";

// All AI endpoints are admin-only (server-enforced 403). These hooks are only mounted on the
// admin AI settings tab (prompt.md §6, §9.7, §10).

const PROVIDERS_KEY = ["ai", "providers"];
const SETTINGS_KEY = ["ai", "settings"];

export function useAiPresets() {
    return useQuery<AiPreset[]>({
        queryKey: ["ai", "presets"],
        queryFn: () => api.get<AiPreset[]>("/ai/presets"),
        staleTime: Infinity, // static templates
    });
}

export function useAiProviders() {
    return useQuery<AiProvider[]>({
        queryKey: PROVIDERS_KEY,
        queryFn: () => api.get<AiProvider[]>("/ai/providers"),
    });
}

export function useCreateProvider() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (body: NewAiProvider) =>
            api.post<{ id: number }>("/ai/providers", body),
        onSuccess: () => qc.invalidateQueries({ queryKey: PROVIDERS_KEY }),
    });
}

export function usePatchProvider() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: ({
            id,
            ...body
        }: {
            id: number;
            name?: string;
            model?: string;
        }) => api.patch<{ ok: boolean }>(`/ai/providers/${id}`, body),
        onSuccess: () => qc.invalidateQueries({ queryKey: PROVIDERS_KEY }),
    });
}

export function useActivateProvider() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (id: number) =>
            api.post<{ ok: boolean }>(`/ai/providers/${id}/activate`),
        onSuccess: () => qc.invalidateQueries({ queryKey: PROVIDERS_KEY }),
    });
}

export function useDeleteProvider() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (id: number) =>
            api.del<{ ok: boolean }>(`/ai/providers/${id}`),
        onSuccess: () => qc.invalidateQueries({ queryKey: PROVIDERS_KEY }),
    });
}

/** Test connection: reports ok/error; never echoes the key. */
export function useTestProvider() {
    return useMutation({
        mutationFn: (id: number) =>
            api.post<TestResult>(`/ai/providers/${id}/test`),
    });
}

export function useAiSettings() {
    return useQuery<AiSettings>({
        queryKey: SETTINGS_KEY,
        queryFn: () => api.get<AiSettings>("/ai/settings"),
    });
}

export function useUpdateAiSettings() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (
            body: Omit<
                AiSettings,
                "tokens_used_today" | "tokens_used_month" | "video_provider_id"
            >,
        ) => api.put<{ ok: boolean }>("/ai/settings", body),
        onSuccess: () => qc.invalidateQueries({ queryKey: SETTINGS_KEY }),
    });
}

/** Point the YouTube video-summary slot at a Gemini provider, or clear it with null (§6a). */
export function useSetVideoProvider() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (providerId: number | null) =>
            api.put<{ ok: boolean }>("/ai/video-provider", {
                provider_id: providerId,
            }),
        onSuccess: () => qc.invalidateQueries({ queryKey: SETTINGS_KEY }),
    });
}
