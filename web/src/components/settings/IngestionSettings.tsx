import { useEffect, useState } from "react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import {
    NumField,
    SETTINGS_TILE_CLASS,
    TileTitle,
} from "@/components/settings/SettingsTile";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { useAutosave } from "@/hooks/useAutosave";
import {
    useIngestionSettings,
    usePurgeRetention,
    useUpdateIngestionSettings,
} from "@/hooks/useSettings";
import type { IngestionSettings as Ingestion } from "@/lib/types";
import { cn } from "@/lib/utils";

/** Ingestion + retention tab (admin-only, prompt.md §8). Concurrency/politeness/interval, retention
 *  policy (starred kept forever), and the SSRF allow-private toggle. Autosaves (debounced) on
 *  every change; server enforces the admin role (§11). */
// biome-ignore lint/complexity/noExcessiveLinesPerFunction: existing baseline
export function IngestionSettings() {
    const settings = useIngestionSettings();
    const update = useUpdateIngestionSettings();
    const purge = usePurgeRetention();
    const [form, setForm] = useState<Ingestion | null>(null);
    const [confirmPurge, setConfirmPurge] = useState(false);

    useEffect(() => {
        if (settings.data) setForm(settings.data);
    }, [settings.data]);

    useAutosave(form, (f) =>
        update.mutate(f, {
            onError: (e) =>
                toast.error(e instanceof Error ? e.message : "Could not save"),
        }),
    );

    if (settings.isLoading || !form)
        return (
            <div className="flex justify-center py-6">
                <Spinner className="size-6" />
            </div>
        );
    if (settings.isError) return <ErrorBanner error={settings.error} />;

    const patch = (p: Partial<Ingestion>) =>
        setForm((f) => (f ? { ...f, ...p } : f));

    const purgeNow = () => {
        purge.mutate(undefined, {
            onSuccess: (r) =>
                toast.success(
                    `Deleted ${r.removed} item${r.removed === 1 ? "" : "s"}`,
                ),
            onError: (e) =>
                toast.error(e instanceof Error ? e.message : "Delete failed"),
        });
    };

    return (
        <div className="space-y-5">
            <div className="space-y-3.5">
                <h3 className="border-b border-border pb-2 text-[13px] font-bold tracking-wide">
                    Fetching
                </h3>
                <div className="grid gap-4 sm:grid-cols-2">
                    <NumField
                        label="Global concurrency"
                        value={form.concurrency}
                        onChange={(v) => patch({ concurrency: v })}
                        min={1}
                    />
                    <NumField
                        label="Per-host delay"
                        value={form.per_host_delay_ms}
                        onChange={(v) => patch({ per_host_delay_ms: v })}
                        min={0}
                        unit="ms"
                    />
                    <NumField
                        label="Fetch timeout"
                        value={form.timeout_secs}
                        onChange={(v) => patch({ timeout_secs: v })}
                        min={1}
                        unit="sec"
                    />
                    <NumField
                        label="Default check interval"
                        description="How often each feed is checked for new items."
                        value={Math.round(form.default_interval_secs / 3600)}
                        onChange={(v) =>
                            patch({ default_interval_secs: v * 3600 })
                        }
                        min={1}
                        max={24}
                        unit="hrs"
                    />
                    <NumField
                        label="Ingest cutoff"
                        description="Skip articles older than this when a feed is first added. 0 fetches everything."
                        value={form.max_item_age_days}
                        onChange={(v) => patch({ max_item_age_days: v })}
                        min={0}
                        unit="days"
                    />
                </div>
                {/* biome-ignore lint/a11y/noLabelWithoutControl: existing baseline */}
                <label
                    className={cn(
                        SETTINGS_TILE_CLASS,
                        "flex cursor-pointer items-center justify-between gap-4",
                    )}
                >
                    <TileTitle
                        title="Allow private / loopback addresses"
                        description="SSRF override - leave off unless you self-host feeds on your LAN."
                    />
                    <Switch
                        checked={form.allow_private}
                        onCheckedChange={(v) => patch({ allow_private: v })}
                        className="shrink-0"
                    />
                </label>
            </div>

            <div className="space-y-3.5">
                <h3 className="border-b border-border pb-2 text-[13px] font-bold tracking-wide">
                    Retention
                </h3>
                <div className="grid gap-4 sm:grid-cols-2">
                    <NumField
                        label="Purge older than"
                        description="0 keeps items forever."
                        value={form.retention_max_age_days}
                        onChange={(v) => patch({ retention_max_age_days: v })}
                        min={0}
                        unit="days"
                    />
                    <NumField
                        label="Keep per feed"
                        description="0 keeps an unlimited number."
                        value={form.retention_max_per_feed}
                        onChange={(v) => patch({ retention_max_per_feed: v })}
                        min={0}
                        unit="items"
                    />
                </div>
            </div>

            <div className="space-y-3 border-border">
                <h3 className="border-b border-border pb-2 text-[13px] font-bold tracking-wide text-destructive">
                    Danger zone
                </h3>
                <div className="flex flex-col gap-3 rounded-lg border border-destructive/30 bg-destructive/5 p-3.5 sm:flex-row sm:items-center sm:justify-between">
                    <TileTitle
                        title="Delete old items"
                        description={
                            <>
                                Starred items are always kept, regardless of
                                retention. "Delete" uses the retention values
                                above - give them a moment to autosave if you
                                just changed them.
                            </>
                        }
                    />
                    <Button
                        variant="destructive"
                        className="shrink-0"
                        disabled={purge.isPending}
                        onClick={() => setConfirmPurge(true)}
                    >
                        {purge.isPending ? "Deleting…" : "Delete"}
                    </Button>
                </div>
            </div>

            <ConfirmDialog
                open={confirmPurge}
                onOpenChange={setConfirmPurge}
                title="Delete old items?"
                description="Delete items older than the saved retention policy right now? Starred items are always kept. This cannot be undone."
                confirmLabel="Delete"
                destructive
                onConfirm={purgeNow}
            />
        </div>
    );
}
