import { useEffect, useState } from "react";
import { Plus, Sparkles, Trash2, Wifi } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Spinner } from "@/components/ui/spinner";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { EmptyState } from "@/components/common/EmptyState";
import { AddProviderModal } from "@/components/settings/AddProviderModal";
import {
  useActivateProvider,
  useAiProviders,
  useAiSettings,
  useDeleteProvider,
  useTestProvider,
  useUpdateAiSettings,
} from "@/hooks/useAi";
import { toast } from "@/stores/toast";
import type { AiProvider } from "@/lib/types";

/** Admin AI tab (prompt.md §9.7): provider manager (active radio · test · delete · key hidden) +
 *  global generation params. Admin-only; the server also enforces the role (§11). */
export function AiSettings() {
  const providers = useAiProviders();
  const [addOpen, setAddOpen] = useState(false);

  return (
    <div className="space-y-8">
      <section className="space-y-3">
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-base font-semibold">AI providers</h2>
            <p className="text-sm text-muted-foreground">One provider is active instance-wide; every user's summaries use it.</p>
          </div>
          <Button size="sm" onClick={() => setAddOpen(true)}>
            <Plus className="size-4" /> Add provider
          </Button>
        </div>

        {providers.isLoading ? (
          <div className="flex justify-center py-6"><Spinner className="size-6" /></div>
        ) : providers.isError ? (
          <ErrorBanner error={providers.error} />
        ) : providers.data && providers.data.length > 0 ? (
          <ul className="space-y-2">
            {providers.data.map((p) => (
              <ProviderRow key={p.id} provider={p} />
            ))}
          </ul>
        ) : (
          <EmptyState
            icon={<Sparkles className="size-8" />}
            title="No AI providers yet"
            description="Add a provider (e.g. an OpenAI-compatible key, or a local Ollama) to enable summaries."
          />
        )}
      </section>

      <GlobalParams />

      <AddProviderModal open={addOpen} onOpenChange={setAddOpen} />
    </div>
  );
}

function ProviderRow({ provider }: { provider: AiProvider }) {
  const activate = useActivateProvider();
  const remove = useDeleteProvider();
  const test = useTestProvider();

  const runTest = () =>
    test.mutate(provider.id, {
      onSuccess: (r) => toast(r.ok ? "Connection OK" : `Test failed: ${r.error ?? "unknown error"}`, r.ok ? "success" : "error"),
      onError: (e) => toast(e instanceof Error ? e.message : "Test failed", "error"),
    });

  const del = () => {
    if (!window.confirm(`Delete provider “${provider.name}”? To rotate a key you delete and re-add.`)) return;
    remove.mutate(provider.id, {
      onSuccess: () => toast("Provider deleted"),
      onError: (e) => toast(e instanceof Error ? e.message : "Could not delete", "error"),
    });
  };

  return (
    <li className="flex flex-col gap-3 rounded-md border border-border p-3 wide:flex-row wide:items-center">
      <label className="flex items-start gap-3">
        <input
          type="radio"
          name="active-provider"
          className="mt-1 size-4 accent-primary"
          checked={provider.is_active}
          onChange={() => activate.mutate(provider.id)}
          aria-label={`Make ${provider.name} the active provider`}
        />
        <span className="min-w-0">
          <span className="flex items-center gap-2">
            <span className="font-medium">{provider.name}</span>
            {provider.is_active && <Badge>active</Badge>}
          </span>
          <span className="block text-xs text-muted-foreground">
            {provider.provider_type} · {provider.api_style === "anthropic" ? "Anthropic" : "OpenAI-compatible"} · {provider.model}
          </span>
          <span className="block text-xs text-muted-foreground">
            {provider.has_key ? "🔒 key saved · hidden" : "no key (local)"}
          </span>
        </span>
      </label>

      <div className="flex items-center gap-2 wide:ml-auto">
        <Button variant="outline" size="sm" disabled={test.isPending} onClick={runTest}>
          {test.isPending ? <Spinner className="size-4" /> : <Wifi className="size-4" />} Test
        </Button>
        <Button variant="ghost" size="icon" aria-label="Delete provider" disabled={remove.isPending} onClick={del}>
          <Trash2 className="size-4" />
        </Button>
      </div>
    </li>
  );
}

function GlobalParams() {
  const settings = useAiSettings();
  const update = useUpdateAiSettings();

  const [form, setForm] = useState({
    max_tokens: 1024,
    temperature: 0.3,
    timeout_secs: 60,
    daily_token_budget: 0,
    monthly_token_budget: 0,
  });

  useEffect(() => {
    if (settings.data) {
      const { max_tokens, temperature, timeout_secs, daily_token_budget, monthly_token_budget } = settings.data;
      setForm({ max_tokens, temperature, timeout_secs, daily_token_budget, monthly_token_budget });
    }
  }, [settings.data]);

  if (settings.isLoading) return <div className="flex justify-center py-6"><Spinner className="size-6" /></div>;
  if (settings.isError) return <ErrorBanner error={settings.error} />;

  const num = (v: string, min: number) => Math.max(min, Number(v) || 0);

  const save = () =>
    update.mutate(form, {
      onSuccess: () => toast("AI settings saved", "success"),
      onError: (e) => toast(e instanceof Error ? e.message : "Could not save", "error"),
    });

  return (
    <section className="space-y-3">
      <div>
        <h2 className="text-base font-semibold">Generation settings</h2>
        <p className="text-sm text-muted-foreground">
          Used today: {settings.data?.tokens_used_today ?? 0} tokens · this month: {settings.data?.tokens_used_month ?? 0}
        </p>
      </div>

      <div className="grid gap-4 sm:grid-cols-2">
        <NumField label="Max tokens" value={form.max_tokens} onChange={(v) => setForm((f) => ({ ...f, max_tokens: num(v, 64) }))} />
        <NumField
          label="Temperature"
          step="0.1"
          value={form.temperature}
          onChange={(v) => setForm((f) => ({ ...f, temperature: Math.min(2, Math.max(0, Number(v) || 0)) }))}
        />
        <NumField label="Timeout (seconds)" value={form.timeout_secs} onChange={(v) => setForm((f) => ({ ...f, timeout_secs: num(v, 5) }))} />
        <NumField
          label="Daily token budget (0 = unlimited)"
          value={form.daily_token_budget}
          onChange={(v) => setForm((f) => ({ ...f, daily_token_budget: num(v, 0) }))}
        />
        <NumField
          label="Monthly token budget (0 = unlimited)"
          value={form.monthly_token_budget}
          onChange={(v) => setForm((f) => ({ ...f, monthly_token_budget: num(v, 0) }))}
        />
      </div>

      <div className="flex justify-end">
        <Button onClick={save} disabled={update.isPending}>
          {update.isPending ? "Saving…" : "Save settings"}
        </Button>
      </div>
    </section>
  );
}

function NumField({
  label,
  value,
  onChange,
  step,
}: {
  label: string;
  value: number;
  onChange: (v: string) => void;
  step?: string;
}) {
  return (
    <div className="space-y-1.5">
      <Label>{label}</Label>
      <Input type="number" step={step} value={value} onChange={(e) => onChange(e.target.value)} />
    </div>
  );
}
