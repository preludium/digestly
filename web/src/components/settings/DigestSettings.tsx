import { Play } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import {
    SETTINGS_TILE_CLASS,
    TileTitle,
} from "@/components/settings/SettingsTile";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { NumberField } from "@/components/ui/number-field";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { useAutosave } from "@/hooks/useAutosave";
import { useCategories } from "@/hooks/useCategories";
import {
    useDigestConfig,
    useRunDigest,
    useUpdateDigestConfig,
} from "@/hooks/useDigest";
import type { PutDigestConfig } from "@/lib/types";
import { cn } from "@/lib/utils";

/** Digest engine tab (admin-only, prompt.md §7, §9.7): enable, cron + human preview, look-back,
 *  categories, AI on/off, and Run-now (runs for all users). Server enforces the admin role (§11). */
// biome-ignore lint/complexity/noExcessiveLinesPerFunction: existing baseline
export function DigestSettings() {
    const config = useDigestConfig();
    const update = useUpdateDigestConfig();
    const run = useRunDigest();
    const categories = useCategories();
    const [runLookbackDays, setRunLookbackDays] = useState<number | null>(null);
    const [confirmRun, setConfirmRun] = useState(false);
    const [runOverride, setRunOverride] = useState<number | undefined>(
        undefined,
    );

    const [form, setForm] = useState<PutDigestConfig>({
        enabled: true,
        cron: "0 5 * * *",
        lookback_days: 1,
        timezone: "UTC",
        categories: null,
        ai_enabled: true,
    });

    useEffect(() => {
        if (config.data) {
            const {
                enabled,
                cron,
                lookback_days,
                timezone,
                categories,
                ai_enabled,
            } = config.data;
            setForm({
                enabled,
                cron,
                lookback_days,
                timezone,
                categories,
                ai_enabled,
            });
        }
    }, [config.data]);

    useAutosave(form, (f) =>
        update.mutate(f, {
            onError: (e) =>
                toast.error(e instanceof Error ? e.message : "Could not save"),
        }),
    );

    if (config.isLoading)
        return (
            <div className="flex justify-center py-6">
                <Spinner className="size-6" />
            </div>
        );
    if (config.isError) return <ErrorBanner error={config.error} />;

    const patch = (p: Partial<PutDigestConfig>) =>
        setForm((f) => ({ ...f, ...p }));

    const allNames = categories.data?.map((c) => c.name) ?? [];
    const includeAll = form.categories === null;
    const isIncluded = (name: string) =>
        includeAll || (form.categories ?? []).includes(name);

    const toggleCategory = (name: string) => {
        // Materialise the "all" set into an explicit list on first toggle so a category can be excluded.
        const current = form.categories ?? allNames;
        const next = current.includes(name)
            ? current.filter((n) => n !== name)
            : [...current, name];
        patch({ categories: next.length === allNames.length ? null : next });
    };

    const runNow = () => {
        const override =
            runLookbackDays == null
                ? undefined
                : Math.min(90, Math.max(1, runLookbackDays));
        setRunOverride(override);
        setConfirmRun(true);
    };

    const doRun = () => {
        run.mutate(runOverride, {
            onSuccess: (s) =>
                toast.success(
                    `Digest ran - ${s.digests} generated, ${s.pushed} pushed`,
                ),
            onError: (e) =>
                toast.error(
                    e instanceof Error ? e.message : "Digest run failed",
                ),
        });
    };

    return (
        <div className="space-y-5">
            <div className="space-y-3.5">
                <h3 className="border-b border-border pb-2 text-[13px] font-bold tracking-wide">
                    Schedule
                </h3>
                {/* biome-ignore lint/a11y/noLabelWithoutControl: existing baseline */}
                <label
                    className={cn(
                        SETTINGS_TILE_CLASS,
                        "flex cursor-pointer items-center justify-between gap-4",
                    )}
                >
                    <TileTitle
                        title="Enable scheduled digests"
                        description="Runs automatically on the schedule below."
                    />
                    <Switch
                        checked={form.enabled}
                        onCheckedChange={(v) => patch({ enabled: v })}
                        className="shrink-0"
                    />
                </label>
                <div className="grid gap-4 sm:grid-cols-2">
                    <div className="space-y-1.5">
                        <Label htmlFor="cron">Schedule</Label>
                        <Input
                            id="cron"
                            value={form.cron}
                            onChange={(e) => patch({ cron: e.target.value })}
                            placeholder="0 9 * * 1"
                        />
                        {config.data?.schedule_preview && (
                            <p className="text-xs text-muted-foreground">
                                {config.data.schedule_preview}
                            </p>
                        )}
                    </div>
                    <div className="space-y-1.5">
                        <Label htmlFor="tz">Timezone</Label>
                        <Input
                            id="tz"
                            value={form.timezone}
                            onChange={(e) =>
                                patch({ timezone: e.target.value })
                            }
                            placeholder="Europe/Warsaw"
                        />
                    </div>
                    <div className="space-y-1.5">
                        <Label htmlFor="lookback">Look-back window</Label>
                        <NumberField
                            id="lookback"
                            value={form.lookback_days}
                            onChange={(v) => patch({ lookback_days: v })}
                            min={1}
                            max={90}
                            unit="days"
                        />
                        <p className="text-xs text-muted-foreground">
                            How far back to gather items each time the digest
                            runs, e.g. 1 day only includes what came in since
                            yesterday.
                        </p>
                    </div>
                </div>
            </div>

            <div className="space-y-3.5">
                <h3 className="border-b border-border pb-2 text-[13px] font-bold tracking-wide">
                    Content
                </h3>

                <fieldset className="space-y-2">
                    <legend className="text-sm font-medium">
                        Included categories
                    </legend>
                    <div className="flex flex-wrap gap-3">
                        {allNames.map((name) => (
                            // biome-ignore lint/a11y/noLabelWithoutControl: existing baseline
                            <label
                                key={name}
                                className="flex items-center gap-2 text-sm"
                            >
                                <Checkbox
                                    checked={isIncluded(name)}
                                    onCheckedChange={() => toggleCategory(name)}
                                />
                                {name}
                            </label>
                        ))}
                    </div>
                </fieldset>

                {/* biome-ignore lint/a11y/noLabelWithoutControl: existing baseline */}
                <label
                    className={cn(
                        SETTINGS_TILE_CLASS,
                        "flex cursor-pointer items-center justify-between gap-4",
                    )}
                >
                    <TileTitle
                        title="Summarize with AI"
                        description="Off falls back to raw grouped titles."
                    />
                    <Switch
                        checked={form.ai_enabled}
                        onCheckedChange={(v) => patch({ ai_enabled: v })}
                        className="shrink-0"
                    />
                </label>

                <div className="flex flex-wrap items-end gap-2 border-border">
                    <div className="space-y-1.5">
                        <Label htmlFor="run-lookback">
                            Look-back for this run
                        </Label>
                        <NumberField
                            id="run-lookback"
                            value={runLookbackDays ?? form.lookback_days}
                            onChange={(v) => setRunLookbackDays(v)}
                            min={1}
                            max={90}
                            unit="days"
                        />
                        <p className="text-xs text-muted-foreground">
                            Just for this one run - doesn't change the saved
                            setting above.
                        </p>
                    </div>
                    <Button
                        variant="outline"
                        className="bg-card"
                        disabled={run.isPending}
                        onClick={runNow}
                    >
                        {run.isPending ? (
                            <Spinner className="size-4" />
                        ) : (
                            <Play className="size-4" />
                        )}{" "}
                        Run digest now
                    </Button>
                </div>
            </div>

            <ConfirmDialog
                open={confirmRun}
                onOpenChange={setConfirmRun}
                title="Run digest now?"
                description={`Run the digest now for every user, using ${runOverride ? `the last ${runOverride} day${runOverride === 1 ? "" : "s"}` : "the configured look-back window"}?`}
                confirmLabel="Run digest"
                onConfirm={doRun}
            />
        </div>
    );
}
