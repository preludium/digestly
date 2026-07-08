import { KeyRound, Pencil, Trash2 } from "lucide-react";
import { ErrorBanner } from "@/components/common/ErrorBanner";
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
import { toast } from "@/stores/toast";

/** Passkey list + add/rename/delete (prompt.md §9.12). Reused by Profile and Onboarding. */
export function PasskeyManager({ compact = false }: { compact?: boolean }) {
  const supported = passkeysSupported();
  const list = usePasskeys(supported);
  const register = useRegisterPasskey();
  const rename = useRenamePasskey();
  const remove = useDeletePasskey();

  const add = async () => {
    const name = window.prompt("Name this passkey (e.g. “MacBook Touch ID”)", "")?.trim();
    // Cancelled the name prompt → don't start the ceremony.
    if (name === undefined) return;
    try {
      await register.mutateAsync(name || undefined);
      toast("Passkey added", "success");
    } catch (e) {
      if (isCancellation(e)) return;
      toast(e instanceof Error ? e.message : "Could not add passkey", "error");
    }
  };

  const doRename = (id: number, current: string) => {
    const name = window.prompt("Rename passkey", current)?.trim();
    if (!name || name === current) return;
    rename.mutate({ id, name }, { onError: (e) => toast(e instanceof Error ? e.message : "Rename failed", "error") });
  };

  const doDelete = (id: number, name: string) => {
    if (!window.confirm(`Delete passkey “${name}”? You won't be able to sign in with it anymore.`)) return;
    remove.mutate(id, {
      onSuccess: () => toast("Passkey removed", "success"),
      onError: (e) => toast(e instanceof Error ? e.message : "Delete failed", "error"),
    });
  };

  if (!supported) {
    return <p className="text-sm text-muted-foreground">This browser doesn't support passkeys.</p>;
  }

  return (
    <div className="space-y-3">
      {list.isError && <ErrorBanner error={list.error} />}

      {list.isLoading ? (
        <Spinner className="size-5" />
      ) : list.data && list.data.length > 0 ? (
        <ul className="divide-y divide-border rounded-md border border-border">
          {list.data.map((pk) => (
            <li key={pk.id} className="flex items-center justify-between gap-3 px-3 py-2">
              <div className="min-w-0">
                <p className="flex items-center gap-2 truncate font-medium">
                  <KeyRound className="size-4 shrink-0 text-muted-foreground" />
                  {pk.name}
                </p>
                <p className="text-xs text-muted-foreground">
                  Added {formatDateTime(pk.created_at)}
                  {pk.last_used_at ? ` · last used ${formatDateTime(pk.last_used_at)}` : " · never used"}
                </p>
              </div>
              <div className="flex shrink-0 gap-1">
                <Button variant="ghost" size="icon" aria-label="Rename passkey" onClick={() => doRename(pk.id, pk.name)}>
                  <Pencil className="size-4" />
                </Button>
                <Button variant="ghost" size="icon" aria-label="Delete passkey" onClick={() => doDelete(pk.id, pk.name)}>
                  <Trash2 className="size-4" />
                </Button>
              </div>
            </li>
          ))}
        </ul>
      ) : (
        !compact && <p className="text-sm text-muted-foreground">No passkeys yet. Add one for passwordless sign-in.</p>
      )}

      <Button variant="outline" size="sm" disabled={register.isPending} onClick={add}>
        {register.isPending ? <Spinner className="size-4" /> : <KeyRound className="size-4" />}
        Add a passkey
      </Button>
    </div>
  );
}
