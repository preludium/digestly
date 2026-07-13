import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api";
import type {
    NotificationConfig,
    PutNotifications,
    TestResult,
} from "@/lib/types";

// Per-user ntfy config (prompt.md §7a, §9.7). Every user (admin or not) has this.

const KEY = ["notifications"];

export function useNotifications() {
    return useQuery<NotificationConfig>({
        queryKey: KEY,
        queryFn: () => api.get<NotificationConfig>("/notifications"),
    });
}

export function useUpdateNotifications() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (body: PutNotifications) =>
            api.put<NotificationConfig>("/notifications", body),
        onSuccess: (data) => qc.setQueryData(KEY, data),
    });
}

/** Send a test push; reports ok/error. Never echoes the token. */
export function useTestNotification() {
    return useMutation({
        mutationFn: () => api.post<TestResult>("/notifications/test"),
    });
}
