import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ApiError, api } from "@/lib/api";
import type { RegistrationStatus, User } from "@/lib/types";

/** Current user; `null` when not authenticated (401). */
export function useMe() {
    return useQuery<User | null>({
        queryKey: ["me"],
        queryFn: async () => {
            try {
                return await api.get<User>("/me");
            } catch (e) {
                if (e instanceof ApiError && e.status === 401) return null;
                throw e;
            }
        },
        retry: false,
        staleTime: 30_000,
    });
}

export function useRegistrationStatus() {
    return useQuery<RegistrationStatus>({
        queryKey: ["registration-status"],
        queryFn: () => api.get<RegistrationStatus>("/auth/registration"),
        staleTime: 60_000,
    });
}

type Credentials = { username: string; password: string };

export function useLogin() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (c: Credentials) => api.post<User>("/auth/login", c),
        onSuccess: (user) => qc.setQueryData(["me"], user),
    });
}

export function useRegister() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (c: Credentials) => api.post<User>("/auth/register", c),
        onSuccess: (user) => qc.setQueryData(["me"], user),
    });
}

export function useLogout() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: () => api.post<{ ok: boolean }>("/auth/logout"),
        onSuccess: () => {
            qc.setQueryData(["me"], null);
            qc.clear();
        },
    });
}

export function useLogoutEverywhere() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: () => api.post<{ ok: boolean }>("/auth/logout-all"),
        onSuccess: () => {
            qc.setQueryData(["me"], null);
            qc.clear();
        },
    });
}

export function useChangePassword() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (body: {
            current_password: string;
            new_password: string;
        }) => api.patch<User>("/me", body),
        onSuccess: (user) => qc.setQueryData(["me"], user),
    });
}

export function useUpdateUsername() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (username: string) => api.patch<User>("/me", { username }),
        onSuccess: (user) => qc.setQueryData(["me"], user),
    });
}

export function useDeleteAccount() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: () => api.del<{ ok: boolean }>("/me"),
        onSuccess: () => {
            qc.setQueryData(["me"], null);
            qc.clear();
        },
    });
}
