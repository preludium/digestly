import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api";
import type { OAuthConnection, OAuthProvider, SyncOutcome } from "@/lib/types";

const KEY = ["oauth", "status"];

/** Per-provider connect/configured status for the current user (§3, §9.7). */
export function useOauthStatus() {
    return useQuery<OAuthConnection[]>({
        queryKey: KEY,
        queryFn: () => api.get<OAuthConnection[]>("/oauth/status"),
    });
}

/** Begin linking: fetch the provider consent URL, then navigate the browser to it. */
export function useOauthConnect() {
    return useMutation({
        mutationFn: async (provider: OAuthProvider) => {
            const { url } = await api.get<{ url: string }>(
                `/oauth/${provider}/authorize`,
            );
            window.location.href = url;
        },
    });
}

/** Repeatable "Sync now" - imports subscriptions, adding only new feeds. */
export function useOauthSync() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: ({
            provider,
            categoryId,
        }: {
            provider: OAuthProvider;
            categoryId?: number;
        }) =>
            api.post<SyncOutcome>(
                `/oauth/${provider}/sync`,
                categoryId ? { category_id: categoryId } : undefined,
            ),
        onSuccess: () => {
            qc.invalidateQueries({ queryKey: KEY });
            qc.invalidateQueries({ queryKey: ["feeds"] });
        },
    });
}

export function useOauthDisconnect() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (provider: OAuthProvider) =>
            api.del<{ ok: boolean }>(`/oauth/${provider}`),
        onSuccess: () => qc.invalidateQueries({ queryKey: KEY }),
    });
}
