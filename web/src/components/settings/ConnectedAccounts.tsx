import { useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { Link2, RefreshCw, Youtube } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { useCategories } from "@/hooks/useCategories";
import { useOauthConnect, useOauthDisconnect, useOauthStatus, useOauthSync } from "@/hooks/useOauth";
import { formatDateTime } from "@/lib/format";
import type { OAuthConnection, OAuthProvider } from "@/lib/types";
import { toast } from "@/stores/toast";

const LABELS: Record<OAuthProvider, string> = { youtube: "YouTube", reddit: "Reddit" };

const OAUTH_ERRORS: Record<string, string> = {
  denied: "Authorization was cancelled.",
  bad_state: "The sign-in link expired — please try again.",
  missing_code: "The provider didn't return an authorization code.",
  exchange_failed: "Could not complete the connection with the provider.",
  store_failed: "Could not save the connection.",
  not_configured: "This provider isn't configured on the server.",
  unknown_provider: "Unknown provider.",
};

/** Connected accounts (prompt.md §3, §9.7 — S4): link YouTube/Reddit and sync subscribed
 *  channels/subreddits into your feeds. Only providers the server has credentials for are shown;
 *  when none are configured this renders nothing. */
export function ConnectedAccounts() {
  const status = useOauthStatus();
  const [params, setParams] = useSearchParams();

  // Surface the OAuth callback result (the server redirects back with ?connected / ?oauth_error).
  useEffect(() => {
    const connected = params.get("connected");
    const error = params.get("oauth_error");
    if (connected) toast(`${LABELS[connected as OAuthProvider] ?? connected} connected`, "success");
    if (error) toast(OAUTH_ERRORS[error] ?? "Connection failed", "error");
    if (connected || error) {
      params.delete("connected");
      params.delete("oauth_error");
      setParams(params, { replace: true });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (status.isLoading) return <Spinner className="size-5" />;
  if (status.isError) return <ErrorBanner error={status.error} />;

  const configured = (status.data ?? []).filter((c) => c.configured);
  if (configured.length === 0) return null; // hidden when the server has no OAuth apps set up

  return (
    <section className="space-y-3">
      <div>
        <h2 className="text-base font-semibold">Connected accounts</h2>
        <p className="text-sm text-muted-foreground">
          Link YouTube or Reddit to import the channels/subreddits you follow. Syncing adds only the
          ones you don't already have — you can run it again anytime.
        </p>
      </div>
      <ul className="space-y-3">
        {configured.map((c) => (
          <ProviderRow key={c.provider} conn={c} />
        ))}
      </ul>
    </section>
  );
}

function ProviderRow({ conn }: { conn: OAuthConnection }) {
  const categories = useCategories();
  const connect = useOauthConnect();
  const disconnect = useOauthDisconnect();
  const sync = useOauthSync();
  const [categoryId, setCategoryId] = useState<number | undefined>(undefined);

  const Icon = conn.provider === "youtube" ? Youtube : Link2;

  const doSync = () =>
    sync.mutate(
      { provider: conn.provider, categoryId },
      {
        onSuccess: (r) => toast(`Added ${r.added}, skipped ${r.skipped} already-added`, "success"),
        onError: (e) => toast(e instanceof Error ? e.message : "Sync failed", "error"),
      },
    );

  return (
    <li className="rounded-md border border-border p-3">
      <div className="flex items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2">
          <Icon className="size-5 shrink-0 text-muted-foreground" />
          <div className="min-w-0">
            <p className="font-medium">{LABELS[conn.provider]}</p>
            <p className="truncate text-xs text-muted-foreground">
              {conn.connected
                ? `${conn.account_label ?? "Connected"}${conn.last_sync_at ? ` · last synced ${formatDateTime(conn.last_sync_at)}` : " · not synced yet"}`
                : "Not connected"}
            </p>
          </div>
        </div>
        {conn.connected ? (
          <Button
            variant="ghost"
            size="sm"
            disabled={disconnect.isPending}
            onClick={() =>
              disconnect.mutate(conn.provider, {
                onSuccess: () => toast(`${LABELS[conn.provider]} disconnected`, "success"),
              })
            }
          >
            Disconnect
          </Button>
        ) : (
          <Button size="sm" disabled={connect.isPending} onClick={() => connect.mutate(conn.provider)}>
            <Link2 className="size-4" /> Connect
          </Button>
        )}
      </div>

      {conn.connected && (
        <div className="mt-3 flex flex-col gap-2 sm:flex-row sm:items-center">
          <Select
            className="sm:w-56"
            aria-label="Import into category"
            value={categoryId ?? ""}
            onChange={(e) => setCategoryId(e.target.value ? Number(e.target.value) : undefined)}
          >
            <option value="">Import into: Other (default)</option>
            {(categories.data ?? []).map((cat) => (
              <option key={cat.id} value={cat.id}>
                Import into: {cat.name}
              </option>
            ))}
          </Select>
          <Button variant="outline" size="sm" disabled={sync.isPending} onClick={doSync}>
            {sync.isPending ? <Spinner className="size-4" /> : <RefreshCw className="size-4" />}
            {sync.isPending ? "Syncing…" : "Sync now"}
          </Button>
        </div>
      )}
    </li>
  );
}
