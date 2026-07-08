import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Spinner } from "@/components/ui/spinner";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { useIngestionSettings, usePurgeRetention, useUpdateIngestionSettings } from "@/hooks/useSettings";
import { toast } from "@/stores/toast";
import type { IngestionSettings as Ingestion } from "@/lib/types";

/** Ingestion + retention tab (admin-only, prompt.md §8). Concurrency/politeness/interval, retention
 *  policy (starred kept forever), and the SSRF allow-private toggle. Server enforces the role (§11). */
export function IngestionSettings() {
  const settings = useIngestionSettings();
  const update = useUpdateIngestionSettings();
  const purge = usePurgeRetention();
  const [form, setForm] = useState<Ingestion | null>(null);

  useEffect(() => {
    if (settings.data) setForm(settings.data);
  }, [settings.data]);

  if (settings.isLoading || !form) return <div className="flex justify-center py-6"><Spinner className="size-6" /></div>;
  if (settings.isError) return <ErrorBanner error={settings.error} />;

  const patch = (p: Partial<Ingestion>) => setForm((f) => (f ? { ...f, ...p } : f));
  const num = (v: string, min: number) => Math.max(min, Number(v) || 0);

  const save = () =>
    update.mutate(form, {
      onSuccess: () => toast("Ingestion settings saved", "success"),
      onError: (e) => toast(e instanceof Error ? e.message : "Could not save", "error"),
    });

  const purgeNow = () => {
    if (
      !window.confirm(
        "Delete items older than the saved retention policy right now? Starred items are always kept. This cannot be undone.",
      )
    )
      return;
    purge.mutate(undefined, {
      onSuccess: (r) => toast(`Deleted ${r.removed} item${r.removed === 1 ? "" : "s"}`, "success"),
      onError: (e) => toast(e instanceof Error ? e.message : "Delete failed", "error"),
    });
  };

  return (
    <div className="space-y-5">
      <div>
        <h2 className="text-base font-semibold">Ingestion &amp; retention</h2>
        <p className="text-sm text-muted-foreground">Global polling behaviour and how long items are kept.</p>
      </div>

      <div className="grid gap-4 sm:grid-cols-2">
        <NumField label="Global concurrency" value={form.concurrency} onChange={(v) => patch({ concurrency: num(v, 1) })} />
        <NumField label="Per-host delay (ms)" value={form.per_host_delay_ms} onChange={(v) => patch({ per_host_delay_ms: num(v, 0) })} />
        <NumField label="Fetch timeout (seconds)" value={form.timeout_secs} onChange={(v) => patch({ timeout_secs: num(v, 1) })} />
        <NumField label="Default interval (seconds)" value={form.default_interval_secs} onChange={(v) => patch({ default_interval_secs: num(v, 60) })} />
        <NumField
          label="Retention: purge older than (days, 0 = keep)"
          value={form.retention_max_age_days}
          onChange={(v) => patch({ retention_max_age_days: num(v, 0) })}
        />
        <NumField
          label="Retention: keep per feed (0 = unlimited)"
          value={form.retention_max_per_feed}
          onChange={(v) => patch({ retention_max_per_feed: num(v, 0) })}
        />
      </div>
      <div className="flex flex-col gap-2 rounded-md border border-border p-3 sm:flex-row sm:items-center sm:justify-between">
        <p className="text-xs text-muted-foreground">
          Starred items are always kept, regardless of retention. "Delete now" uses the retention values
          saved above — save your changes first if you just edited them.
        </p>
        <Button variant="outline" disabled={purge.isPending} onClick={purgeNow}>
          {purge.isPending ? "Deleting…" : "Delete now"}
        </Button>
      </div>

      <label className="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          className="size-4 accent-primary"
          checked={form.allow_private}
          onChange={(e) => patch({ allow_private: e.target.checked })}
        />
        Allow fetching private / loopback addresses (SSRF override — leave off unless you self-host feeds on your LAN)
      </label>

      <div className="flex justify-end">
        <Button onClick={save} disabled={update.isPending}>
          {update.isPending ? "Saving…" : "Save settings"}
        </Button>
      </div>
    </div>
  );
}

function NumField({ label, value, onChange }: { label: string; value: number; onChange: (v: string) => void }) {
  return (
    <div className="space-y-1.5">
      <Label>{label}</Label>
      <Input type="number" value={value} onChange={(e) => onChange(e.target.value)} />
    </div>
  );
}
