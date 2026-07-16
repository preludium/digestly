import { Link2, Play, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { toast } from "sonner";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import {
    SETTINGS_TILE_CLASS,
    TileTitle,
} from "@/components/settings/SettingsTile";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
} from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";
import { useCategories } from "@/hooks/useCategories";
import {
    useOauthConnect,
    useOauthDisconnect,
    useOauthStatus,
    useOauthSync,
} from "@/hooks/useOauth";
import { formatDateTime, relativeTime } from "@/lib/format";
import type { OAuthConnection, OAuthProvider } from "@/lib/types";

const LABELS: Record<OAuthProvider, string> = {
    youtube: "YouTube",
    reddit: "Reddit",
};

const OAUTH_ERRORS: Record<string, string> = {
    denied: "Authorization was cancelled.",
    bad_state: "The sign-in link expired - please try again.",
    missing_code: "The provider didn't return an authorization code.",
    exchange_failed: "Could not complete the connection with the provider.",
    store_failed: "Could not save the connection.",
    not_configured: "This provider isn't configured on the server.",
    unknown_provider: "Unknown provider.",
};

/** Connected accounts (prompt.md §3, §9.7 - S4): link YouTube/Reddit and sync subscribed
 *  channels/subreddits into your feeds. Only providers the server has credentials for are shown;
 *  when none are configured this renders nothing. */
export function ConnectedAccounts() {
    const status = useOauthStatus();
    const [params, setParams] = useSearchParams();

    // Surface the OAuth callback result (the server redirects back with ?connected / ?oauth_error).
    // biome-ignore lint/correctness/useExhaustiveDependencies: existing baseline
    useEffect(() => {
        const connected = params.get("connected");
        const error = params.get("oauth_error");
        if (connected)
            toast.success(
                `${LABELS[connected as OAuthProvider] ?? connected} connected`,
            );
        if (error) toast.error(OAUTH_ERRORS[error] ?? "Connection failed");
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
        <div className="space-y-3.5">
            <h3 className="border-b border-border pb-2 text-[13px] font-bold tracking-wide">
                Connected accounts
            </h3>
            <p className="text-[13px] text-muted-foreground">
                Link YouTube or Reddit to import the channels/subreddits you
                follow. Syncing adds only the ones you don't already have - you
                can run it again anytime.
            </p>
            <p className="text-[13px] text-muted-foreground">
                A linked Reddit account is also used to fetch subreddit posts
                for this instance - Reddit throttles anonymous requests hard, so
                scores and comment counts go missing without it. Only public
                subreddit listings are ever requested; nothing is read from or
                posted to your account.
            </p>
            <ul className="space-y-3">
                {configured.map((c) => (
                    <ProviderRow key={c.provider} conn={c} />
                ))}
            </ul>
        </div>
    );
}

function ProviderRow({ conn }: { conn: OAuthConnection }) {
    const categories = useCategories();
    const connect = useOauthConnect();
    const disconnect = useOauthDisconnect();
    const sync = useOauthSync();
    const [categoryId, setCategoryId] = useState<number | undefined>(undefined);
    const [showFullSync, setShowFullSync] = useState(false);

    const Icon = conn.provider === "youtube" ? Play : Link2;

    const doSync = () =>
        sync.mutate(
            { provider: conn.provider, categoryId },
            {
                onSuccess: (r) =>
                    toast.success(
                        `Added ${r.added}, skipped ${r.skipped} already-added`,
                    ),
                onError: (e) =>
                    toast.error(e instanceof Error ? e.message : "Sync failed"),
            },
        );

    // Only show the account label if it says something the provider name doesn't already (avoids
    // "YouTube · YouTube · last synced …" when no distinct account name is available).
    const accountLabel =
        conn.account_label &&
        conn.account_label.toLowerCase() !== LABELS[conn.provider].toLowerCase()
            ? conn.account_label
            : null;

    return (
        <li className={SETTINGS_TILE_CLASS}>
            <div className="flex items-center justify-between gap-3">
                <div className="flex min-w-0 items-center gap-2">
                    <Icon className="size-5 shrink-0 text-muted-foreground" />
                    <TileTitle
                        title={LABELS[conn.provider]}
                        description={
                            !conn.connected ? (
                                "Not connected"
                            ) : (
                                <>
                                    {accountLabel && `${accountLabel} · `}
                                    {conn.last_sync_at ? (
                                        <>
                                            Last synced{" "}
                                            <button
                                                type="button"
                                                onClick={(e) => {
                                                    e.preventDefault();
                                                    setShowFullSync((v) => !v);
                                                }}
                                                className="underline decoration-border decoration-dotted underline-offset-4"
                                            >
                                                {showFullSync
                                                    ? formatDateTime(
                                                          conn.last_sync_at,
                                                      )
                                                    : relativeTime(
                                                          conn.last_sync_at,
                                                      )}
                                            </button>
                                        </>
                                    ) : (
                                        "Not synced yet"
                                    )}
                                </>
                            )
                        }
                    />
                </div>
                {conn.connected ? (
                    <Button
                        variant="ghost"
                        size="sm"
                        className="text-destructive hover:bg-destructive/10"
                        disabled={disconnect.isPending}
                        onClick={() =>
                            disconnect.mutate(conn.provider, {
                                onSuccess: () =>
                                    toast.success(
                                        `${LABELS[conn.provider]} disconnected`,
                                    ),
                            })
                        }
                    >
                        Disconnect
                    </Button>
                ) : (
                    <Button
                        size="sm"
                        disabled={connect.isPending}
                        onClick={() => connect.mutate(conn.provider)}
                    >
                        <Link2 className="size-4" /> Connect
                    </Button>
                )}
            </div>

            {conn.connected && (
                <div className="mt-3 flex flex-col gap-2 sm:flex-row sm:items-center">
                    <Label
                        htmlFor={`import-cat-${conn.provider}`}
                        className="shrink-0 text-xs text-muted-foreground"
                    >
                        Import into
                    </Label>
                    <Select
                        value={
                            categoryId === undefined ? "" : String(categoryId)
                        }
                        onValueChange={(v) =>
                            setCategoryId(v ? Number(v) : undefined)
                        }
                    >
                        <SelectTrigger
                            id={`import-cat-${conn.provider}`}
                            className="sm:w-56"
                        >
                            <SelectValue placeholder="Other (default)" />
                        </SelectTrigger>
                        <SelectContent>
                            {(categories.data ?? []).map((cat) => (
                                <SelectItem key={cat.id} value={String(cat.id)}>
                                    {cat.name}
                                </SelectItem>
                            ))}
                        </SelectContent>
                    </Select>
                    <Button
                        variant="outline"
                        size="sm"
                        disabled={sync.isPending}
                        onClick={doSync}
                    >
                        {sync.isPending ? (
                            <Spinner className="size-4" />
                        ) : (
                            <RefreshCw className="size-4" />
                        )}
                        {sync.isPending ? "Syncing…" : "Sync now"}
                    </Button>
                </div>
            )}
        </li>
    );
}
