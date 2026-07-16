import { KeyRound, Pencil, Trash2 } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { NameDialog } from "@/components/common/NameDialog";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import {
    useDeletePasskey,
    usePasskeys,
    useRegisterPasskey,
    useRenamePasskey,
} from "@/hooks/usePasskeys";
import { formatDateTime } from "@/lib/format";
import { isCancellation, passkeysSupported } from "@/lib/webauthn";

/** Passkey list + add/rename/delete (prompt.md §9.12). Reused by Profile and Onboarding. */
// biome-ignore lint/complexity/noExcessiveLinesPerFunction: existing baseline
export function PasskeyManager({ compact = false }: { compact?: boolean }) {
    const supported = passkeysSupported();
    const list = usePasskeys(supported);
    const register = useRegisterPasskey();
    const rename = useRenamePasskey();
    const remove = useDeletePasskey();

    const [adding, setAdding] = useState(false);
    const [renaming, setRenaming] = useState<{
        id: number;
        name: string;
    } | null>(null);
    const [deleting, setDeleting] = useState<{
        id: number;
        name: string;
    } | null>(null);

    const handleAdd = async (name: string) => {
        try {
            await register.mutateAsync(name || undefined);
            toast.success("Passkey added");
        } catch (e) {
            if (isCancellation(e)) return;
            toast.error(
                e instanceof Error ? e.message : "Could not add passkey",
            );
        }
    };

    const handleRename = (name: string) => {
        if (!renaming) return;
        rename.mutate(
            { id: renaming.id, name },
            {
                onError: (e) =>
                    toast.error(
                        e instanceof Error ? e.message : "Rename failed",
                    ),
            },
        );
    };

    const handleDelete = () => {
        if (!deleting) return;
        remove.mutate(deleting.id, {
            onSuccess: () => toast.success("Passkey removed"),
            onError: (e) =>
                toast.error(e instanceof Error ? e.message : "Delete failed"),
        });
    };

    if (!supported) {
        return (
            <p className="text-sm text-muted-foreground">
                This browser doesn't support passkeys.
            </p>
        );
    }

    return (
        <div className="space-y-3">
            {list.isError && <ErrorBanner error={list.error} />}

            {list.isLoading ? (
                <Spinner className="size-5" />
            ) : list.data && list.data.length > 0 ? (
                <ul className="divide-y divide-border rounded-md border border-border">
                    {list.data.map((pk) => (
                        <li
                            key={pk.id}
                            className="flex items-center justify-between gap-3 px-3 py-2"
                        >
                            <div className="min-w-0">
                                <p className="flex items-center gap-2 truncate font-medium">
                                    <KeyRound className="size-4 shrink-0 text-muted-foreground" />
                                    {pk.name}
                                </p>
                                <p className="text-xs text-muted-foreground">
                                    Added {formatDateTime(pk.created_at)}
                                    {pk.last_used_at
                                        ? ` · last used ${formatDateTime(pk.last_used_at)}`
                                        : " · never used"}
                                </p>
                            </div>
                            <div className="flex shrink-0 gap-1">
                                <Button
                                    variant="ghost"
                                    size="icon"
                                    aria-label="Rename passkey"
                                    onClick={() =>
                                        setRenaming({
                                            id: pk.id,
                                            name: pk.name,
                                        })
                                    }
                                >
                                    <Pencil className="size-4" />
                                </Button>
                                <Button
                                    variant="ghost"
                                    size="icon"
                                    aria-label="Delete passkey"
                                    onClick={() =>
                                        setDeleting({
                                            id: pk.id,
                                            name: pk.name,
                                        })
                                    }
                                >
                                    <Trash2 className="size-4" />
                                </Button>
                            </div>
                        </li>
                    ))}
                </ul>
            ) : (
                !compact && (
                    <p className="text-sm text-muted-foreground">
                        No passkeys yet. Add one for passwordless sign-in.
                    </p>
                )
            )}

            <Button
                variant="outline"
                size="sm"
                disabled={register.isPending}
                onClick={() => setAdding(true)}
            >
                {register.isPending ? (
                    <Spinner className="size-4" />
                ) : (
                    <KeyRound className="size-4" />
                )}
                Add a passkey
            </Button>

            <NameDialog
                open={adding}
                onOpenChange={setAdding}
                title="Add a passkey"
                label="Passkey name"
                placeholder='e.g. "MacBook Touch ID"'
                submitLabel="Continue"
                allowEmpty
                onSubmit={handleAdd}
            />
            <NameDialog
                open={!!renaming}
                onOpenChange={(v) => !v && setRenaming(null)}
                title="Rename passkey"
                label="Name"
                initialValue={renaming?.name ?? ""}
                submitLabel="Rename"
                onSubmit={handleRename}
            />
            <ConfirmDialog
                open={!!deleting}
                onOpenChange={(v) => !v && setDeleting(null)}
                title={`Delete passkey "${deleting?.name ?? ""}"?`}
                description="You won't be able to sign in with it anymore."
                confirmLabel="Delete"
                destructive
                onConfirm={handleDelete}
            />
        </div>
    );
}
