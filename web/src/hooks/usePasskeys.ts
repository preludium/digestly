import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api";
import type { Passkey, User } from "@/lib/types";
import {
    runAuthentication,
    runConditionalAuthentication,
    runRegistration,
} from "@/lib/webauthn";

type Ceremony<T> = { ceremony_id: string; options: T };

/** The current user's registered passkeys (§9.12). */
export function usePasskeys(enabled = true) {
    return useQuery<Passkey[]>({
        queryKey: ["passkeys"],
        queryFn: () => api.get<Passkey[]>("/passkeys"),
        enabled,
    });
}

/** Register a new passkey: fetch options → browser ceremony → verify. */
export function useRegisterPasskey() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: async (name?: string) => {
            const { ceremony_id, options } = await api.post<
                Ceremony<{ publicKey: unknown }>
            >("/passkeys/register/options");
            // `options` is a webauthn-rs CreationChallengeResponse: `{ publicKey: {...} }`.
            const credential = await runRegistration(
                (options as { publicKey: never }).publicKey,
            );
            return api.post<Passkey>("/passkeys/register/verify", {
                ceremony_id,
                credential,
                name,
            });
        },
        onSuccess: () => qc.invalidateQueries({ queryKey: ["passkeys"] }),
    });
}

export function useRenamePasskey() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: ({ id, name }: { id: number; name: string }) =>
            api.patch<{ ok: boolean }>(`/passkeys/${id}`, { name }),
        onSuccess: () => qc.invalidateQueries({ queryKey: ["passkeys"] }),
    });
}

export function useDeletePasskey() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (id: number) => api.del<{ ok: boolean }>(`/passkeys/${id}`),
        onSuccess: () => qc.invalidateQueries({ queryKey: ["passkeys"] }),
    });
}

/** Passwordless sign-in with a passkey (username-first). */
export function usePasskeyLogin() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: async (username: string) => {
            const { ceremony_id, options } = await api.post<
                Ceremony<{ publicKey: unknown }>
            >("/auth/passkey/login/options", { username });
            const credential = await runAuthentication(
                (options as { publicKey: never }).publicKey,
            );
            return api.post<User>("/auth/passkey/login/verify", {
                ceremony_id,
                credential,
            });
        },
        onSuccess: (user) => qc.setQueryData(["me"], user),
    });
}

/**
 * Discoverable (Conditional UI / autofill) sign-in. Kicks off a background WebAuthn request that
 * primes the browser's autofill with saved passkeys; resolves once the user picks one. Pass an
 * `AbortSignal` so a manual submit can cancel the pending request.
 */
export function useDiscoverablePasskeyLogin() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: async (signal?: AbortSignal) => {
            const { ceremony_id, options } = await api.post<
                Ceremony<{ publicKey: unknown }>
            >("/auth/passkey/discoverable/login/options");
            const credential = await runConditionalAuthentication(
                (options as { publicKey: never }).publicKey,
                signal,
            );
            return api.post<User>("/auth/passkey/discoverable/login/verify", {
                ceremony_id,
                credential,
            });
        },
        onSuccess: (user) => qc.setQueryData(["me"], user),
    });
}
