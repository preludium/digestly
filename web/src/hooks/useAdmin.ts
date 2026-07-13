import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api";
import type { AdminSettings, AdminUser, Role } from "@/lib/types";

export function useUsers() {
    return useQuery<AdminUser[]>({
        queryKey: ["admin", "users"],
        queryFn: () => api.get<AdminUser[]>("/admin/users"),
    });
}

export function useUpdateUser() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: ({
            id,
            ...body
        }: {
            id: number;
            role?: Role;
            disabled?: boolean;
        }) => api.patch<{ ok: boolean }>(`/admin/users/${id}`, body),
        onSuccess: () => qc.invalidateQueries({ queryKey: ["admin", "users"] }),
    });
}

export function useDeleteUser() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (id: number) =>
            api.del<{ ok: boolean }>(`/admin/users/${id}`),
        onSuccess: () => qc.invalidateQueries({ queryKey: ["admin", "users"] }),
    });
}

export function useAdminSettings() {
    return useQuery<AdminSettings>({
        queryKey: ["admin", "settings"],
        queryFn: () => api.get<AdminSettings>("/admin/settings"),
    });
}

export function useUpdateAdminSettings() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (body: AdminSettings) =>
            api.put<AdminSettings>("/admin/settings", body),
        onSuccess: () => {
            qc.invalidateQueries({ queryKey: ["admin", "settings"] });
            qc.invalidateQueries({ queryKey: ["registration-status"] });
        },
    });
}
