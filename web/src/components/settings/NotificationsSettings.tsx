import { Wifi } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import {
    SETTINGS_TILE_CLASS,
    TileTitle,
} from "@/components/common/SettingsTile";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
} from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { useAutosave } from "@/hooks/useAutosave";
import {
    useNotifications,
    useTestNotification,
    useUpdateNotifications,
} from "@/hooks/useNotifications";
import type { PutNotifications } from "@/lib/types";
import { cn } from "@/lib/utils";

/** Notifications (ntfy) tab (prompt.md §7a, §9.7) - every user configures their own push channel:
 *  server URL, topic, write-only auth token, priority, per-event toggles. Autosaves (debounced)
 *  on every change; a Test button sends a one-off push. */
export function NotificationsSettings() {
    const config = useNotifications();
    const update = useUpdateNotifications();
    const test = useTestNotification();

    const [form, setForm] = useState({
        ntfy_server_url: "",
        ntfy_topic: "",
        ntfy_priority: 3,
        notify_on_digest: true,
        notify_on_feed_health: true,
    });
    const [token, setToken] = useState("");
    const [hasToken, setHasToken] = useState(false);

    useEffect(() => {
        if (config.data) {
            const c = config.data;
            setForm({
                ntfy_server_url: c.ntfy_server_url ?? "",
                ntfy_topic: c.ntfy_topic ?? "",
                ntfy_priority: c.ntfy_priority,
                notify_on_digest: c.notify_on_digest,
                notify_on_feed_health: c.notify_on_feed_health,
            });
            setHasToken(c.has_token);
            setToken("");
        }
    }, [config.data]);

    useAutosave({ ...form, token }, ({ token: tok, ...f }) => {
        const body: PutNotifications = {
            ntfy_server_url: f.ntfy_server_url.trim() || null,
            ntfy_topic: f.ntfy_topic.trim() || null,
            ntfy_priority: f.ntfy_priority,
            notify_on_digest: f.notify_on_digest,
            notify_on_feed_health: f.notify_on_feed_health,
        };
        // Only send the token when the user typed a new one (write-only, never round-tripped).
        if (tok.trim()) body.auth_token = tok.trim();
        update.mutate(body, {
            onSuccess: () => {
                if (tok.trim()) {
                    setHasToken(true);
                    setToken("");
                }
            },
            onError: (e) =>
                toast.error(e instanceof Error ? e.message : "Could not save"),
        });
    });

    if (config.isLoading)
        return (
            <div className="flex justify-center py-6">
                <Spinner className="size-6" />
            </div>
        );
    if (config.isError) return <ErrorBanner error={config.error} />;

    const patch = (p: Partial<typeof form>) => setForm((f) => ({ ...f, ...p }));

    const clearToken = () =>
        update.mutate(
            { auth_token: "" },
            {
                onSuccess: () => {
                    toast("Auth token cleared");
                    setHasToken(false);
                },
                onError: (e) =>
                    toast.error(
                        e instanceof Error
                            ? e.message
                            : "Could not clear token",
                    ),
            },
        );

    const runTest = () =>
        test.mutate(undefined, {
            onSuccess: (r) =>
                r.ok
                    ? toast.success("Test push sent")
                    : toast.error(`Test failed: ${r.error ?? "unknown error"}`),
            onError: (e) =>
                toast.error(e instanceof Error ? e.message : "Test failed"),
        });

    return (
        <div className="space-y-5">
            <div className="space-y-3.5">
                <h3 className="border-b border-border pb-2 text-[13px] font-bold tracking-wide">
                    Channel
                </h3>
                <p className="text-[13px] text-muted-foreground">
                    Digestly pushes to your own ntfy server (self-hosted or
                    ntfy.sh).
                </p>
                <div className="grid gap-4 sm:grid-cols-2">
                    <div className="space-y-1.5">
                        <Label htmlFor="ntfy-url">Server URL</Label>
                        <Input
                            id="ntfy-url"
                            value={form.ntfy_server_url}
                            onChange={(e) =>
                                patch({ ntfy_server_url: e.target.value })
                            }
                            placeholder="https://ntfy.sh"
                        />
                    </div>
                    <div className="space-y-1.5">
                        <Label htmlFor="ntfy-topic">Topic</Label>
                        <Input
                            id="ntfy-topic"
                            value={form.ntfy_topic}
                            onChange={(e) =>
                                patch({ ntfy_topic: e.target.value })
                            }
                            placeholder="my-digestly"
                        />
                    </div>
                    <div className="space-y-1.5">
                        <Label htmlFor="ntfy-token">
                            Auth token (optional)
                        </Label>
                        <Input
                            id="ntfy-token"
                            type="password"
                            value={token}
                            onChange={(e) => setToken(e.target.value)}
                            placeholder={
                                hasToken ? "🔒 token saved · hidden" : "tk_*** "
                            }
                            autoComplete="off"
                        />
                        {hasToken && (
                            <button
                                type="button"
                                className="text-xs text-muted-foreground underline"
                                onClick={clearToken}
                            >
                                Clear saved token
                            </button>
                        )}
                    </div>
                    <div className="space-y-1.5">
                        <Label htmlFor="ntfy-priority">Priority</Label>
                        <Select
                            value={String(form.ntfy_priority)}
                            onValueChange={(v) =>
                                patch({ ntfy_priority: Number(v) })
                            }
                        >
                            <SelectTrigger id="ntfy-priority">
                                <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                                <SelectItem value="1">Min (1)</SelectItem>
                                <SelectItem value="2">Low (2)</SelectItem>
                                <SelectItem value="3">Default (3)</SelectItem>
                                <SelectItem value="4">High (4)</SelectItem>
                                <SelectItem value="5">Max (5)</SelectItem>
                            </SelectContent>
                        </Select>
                    </div>
                </div>
            </div>

            <fieldset className="space-y-3">
                <legend className="w-full border-b border-border pb-2 text-[13px] font-bold tracking-wide">
                    Notify me
                </legend>
                <NotifyTile
                    title="After each digest"
                    description="A push the moment your scheduled digest is ready."
                    checked={form.notify_on_digest}
                    onCheckedChange={(v) => patch({ notify_on_digest: v })}
                />
                <NotifyTile
                    title="On feed health issues"
                    description="An alert when one of your feeds starts failing."
                    checked={form.notify_on_feed_health}
                    onCheckedChange={(v) => patch({ notify_on_feed_health: v })}
                />
            </fieldset>

            <div className="flex items-center justify-end gap-2">
                <Button
                    variant="outline"
                    disabled={test.isPending}
                    onClick={runTest}
                >
                    {test.isPending ? (
                        <Spinner className="size-4" />
                    ) : (
                        <Wifi className="size-4" />
                    )}{" "}
                    Test
                </Button>
            </div>
        </div>
    );
}

function NotifyTile({
    title,
    description,
    checked,
    onCheckedChange,
}: {
    title: string;
    description: string;
    checked: boolean;
    onCheckedChange: (v: boolean) => void;
}) {
    return (
        // biome-ignore lint/a11y/noLabelWithoutControl: existing baseline
        <label
            className={cn(
                SETTINGS_TILE_CLASS,
                "flex cursor-pointer items-center justify-between gap-4",
            )}
        >
            <TileTitle title={title} description={description} />
            <Switch
                checked={checked}
                onCheckedChange={onCheckedChange}
                className="shrink-0"
            />
        </label>
    );
}
