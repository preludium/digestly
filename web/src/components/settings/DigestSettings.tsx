import { useEffect, useState } from "react";
import { Play } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Spinner } from "@/components/ui/spinner";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { useDigestConfig, useRunDigest, useUpdateDigestConfig } from "@/hooks/useDigest";
import { useCategories } from "@/hooks/useCategories";
import { toast } from "@/stores/toast";
import type { PutDigestConfig } from "@/lib/types";

/** Digest engine tab (admin-only, prompt.md §7, §9.7): enable, cron + human preview, look-back,
 *  categories, AI on/off, and Run-now (runs for all users). Server enforces the admin role (§11). */
export function DigestSettings() {
  const config = useDigestConfig();
  const update = useUpdateDigestConfig();
  const run = useRunDigest();
  const categories = useCategories();
  const [runLookbackDays, setRunLookbackDays] = useState("");

  const [form, setForm] = useState<PutDigestConfig>({
    enabled: true,
    cron: "0 9 * * *",
    lookback_days: 1,
    timezone: "UTC",
    categories: null,
    ai_enabled: true,
  });

  useEffect(() => {
    if (config.data) {
      const { enabled, cron, lookback_days, timezone, categories, ai_enabled } = config.data;
      setForm({ enabled, cron, lookback_days, timezone, categories, ai_enabled });
    }
  }, [config.data]);

  if (config.isLoading) return <div className="flex justify-center py-6"><Spinner className="size-6" /></div>;
  if (config.isError) return <ErrorBanner error={config.error} />;

  const patch = (p: Partial<PutDigestConfig>) => setForm((f) => ({ ...f, ...p }));

  const allNames = categories.data?.map((c) => c.name) ?? [];
  const includeAll = form.categories === null;
  const isIncluded = (name: string) => includeAll || (form.categories ?? []).includes(name);

  const toggleCategory = (name: string) => {
    // Materialise the "all" set into an explicit list on first toggle so a category can be excluded.
    const current = form.categories ?? allNames;
    const next = current.includes(name) ? current.filter((n) => n !== name) : [...current, name];
    patch({ categories: next.length === allNames.length ? null : next });
  };

  const save = () =>
    update.mutate(form, {
      onSuccess: () => toast("Digest settings saved", "success"),
      onError: (e) => toast(e instanceof Error ? e.message : "Could not save", "error"),
    });

  const runNow = () => {
    const trimmed = runLookbackDays.trim();
    const override = trimmed === "" ? undefined : Math.min(90, Math.max(1, Number(trimmed) || 1));
    const label = override ? `the last ${override} day${override === 1 ? "" : "s"}` : "the configured look-back window";
    if (!window.confirm(`Run the digest now for every user, using ${label}?`)) return;
    run.mutate(override, {
      onSuccess: (s) => toast(`Digest ran — ${s.digests} generated, ${s.pushed} pushed`, "success"),
      onError: (e) => toast(e instanceof Error ? e.message : "Digest run failed", "error"),
    });
  };

  return (
    <div className="space-y-5">
      <div>
        <h2 className="text-base font-semibold">Digest engine</h2>
        <p className="text-sm text-muted-foreground">
          One global schedule builds every user their own category-grouped digest. {config.data?.schedule_preview}
        </p>
      </div>

      <label className="flex items-center gap-2 text-sm">
        <input type="checkbox" className="size-4 accent-primary" checked={form.enabled} onChange={(e) => patch({ enabled: e.target.checked })} />
        Enable scheduled digests
      </label>

      <div className="grid gap-4 sm:grid-cols-2">
        <div className="space-y-1.5">
          <Label htmlFor="cron">Schedule (cron)</Label>
          <Input id="cron" value={form.cron} onChange={(e) => patch({ cron: e.target.value })} placeholder="0 9 * * 1" />
          <p className="text-xs text-muted-foreground">minute hour day-of-month month day-of-week (e.g. “0 9 * * 1” = Monday 09:00)</p>
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="tz">Timezone</Label>
          <Input id="tz" value={form.timezone} onChange={(e) => patch({ timezone: e.target.value })} placeholder="Europe/Warsaw" />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="lookback">Look-back window (days)</Label>
          <Input
            id="lookback"
            type="number"
            min={1}
            max={90}
            value={form.lookback_days}
            onChange={(e) => patch({ lookback_days: Math.min(90, Math.max(1, Number(e.target.value) || 1)) })}
          />
        </div>
      </div>

      <label className="flex items-center gap-2 text-sm">
        <input type="checkbox" className="size-4 accent-primary" checked={form.ai_enabled} onChange={(e) => patch({ ai_enabled: e.target.checked })} />
        Summarize with AI (off → raw grouped titles)
      </label>

      <fieldset className="space-y-2">
        <legend className="text-sm font-medium">Included categories</legend>
        <p className="text-xs text-muted-foreground">Applied to each user's own categories by name. All selected = every category.</p>
        <div className="flex flex-wrap gap-3">
          {allNames.map((name) => (
            <label key={name} className="flex items-center gap-2 text-sm">
              <input type="checkbox" className="size-4 accent-primary" checked={isIncluded(name)} onChange={() => toggleCategory(name)} />
              {name}
            </label>
          ))}
        </div>
      </fieldset>

      <div className="flex flex-col gap-3 border-t border-border pt-4 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex items-end gap-2">
          <div className="space-y-1.5">
            <Label htmlFor="run-lookback" className="text-xs text-muted-foreground">
              Custom look-back for this run (days, optional)
            </Label>
            <Input
              id="run-lookback"
              type="number"
              min={1}
              max={90}
              placeholder={String(form.lookback_days)}
              value={runLookbackDays}
              onChange={(e) => setRunLookbackDays(e.target.value)}
              className="w-24"
            />
          </div>
          <Button variant="outline" disabled={run.isPending} onClick={runNow}>
            {run.isPending ? <Spinner className="size-4" /> : <Play className="size-4" />} Run digest now
          </Button>
        </div>
        <Button onClick={save} disabled={update.isPending}>
          {update.isPending ? "Saving…" : "Save settings"}
        </Button>
      </div>
    </div>
  );
}
