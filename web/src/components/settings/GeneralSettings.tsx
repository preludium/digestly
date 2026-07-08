import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select } from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { useSettings, useUpdateSettings } from "@/hooks/useSettings";
import { useUiStore } from "@/stores/ui";
import { toast } from "@/stores/toast";
import type { UserSettings } from "@/lib/types";

type Form = Omit<UserSettings, "onboarded">;

/** General preferences tab (prompt.md §9.7) with a dirty-state save bar. Theme + density also
 *  update the local UI store immediately for a live preview; all fields persist on save. */
export function GeneralSettings() {
  const settings = useSettings();
  const update = useUpdateSettings();
  const setTheme = useUiStore((s) => s.setTheme);
  const setDensity = useUiStore((s) => s.setDensity);

  const [form, setForm] = useState<Form | null>(null);

  useEffect(() => {
    if (settings.data) {
      const { onboarded: _o, ...rest } = settings.data;
      setForm(rest);
    }
  }, [settings.data]);

  const dirty = useMemo(() => {
    if (!settings.data || !form) return false;
    const { onboarded: _o, ...saved } = settings.data;
    return JSON.stringify(saved) !== JSON.stringify(form);
  }, [settings.data, form]);

  if (settings.isLoading || !form) return <div className="flex justify-center py-6"><Spinner className="size-6" /></div>;
  if (settings.isError) return <ErrorBanner error={settings.error} />;

  const patch = (p: Partial<Form>) => setForm((f) => (f ? { ...f, ...p } : f));

  const save = () =>
    update.mutate(form, {
      onSuccess: () => toast("Preferences saved", "success"),
      onError: (e) => toast(e instanceof Error ? e.message : "Could not save", "error"),
    });

  const reset = () => {
    if (!settings.data) return;
    const { onboarded: _o, ...rest } = settings.data;
    setForm(rest);
    setTheme(rest.theme);
    setDensity(rest.density);
  };

  return (
    <div className="space-y-5">
      <div>
        <h2 className="text-base font-semibold">General</h2>
        <p className="text-sm text-muted-foreground">Reading preferences for your account.</p>
      </div>

      <div className="grid gap-4 sm:grid-cols-2">
        <Field label="Theme">
          <Select
            value={form.theme}
            onChange={(e) => {
              const v = e.target.value as Form["theme"];
              patch({ theme: v });
              setTheme(v); // live preview
            }}
          >
            <option value="dark">Dark</option>
            <option value="light">Light</option>
          </Select>
        </Field>
        <Field label="List density">
          <Select
            value={form.density}
            onChange={(e) => {
              const v = e.target.value as Form["density"];
              patch({ density: v });
              setDensity(v);
            }}
          >
            <option value="normal">Normal</option>
            <option value="compact">Compact</option>
          </Select>
        </Field>
        <Field label="Default sort">
          <Select value={form.sort} onChange={(e) => patch({ sort: e.target.value as Form["sort"] })}>
            <option value="new">Newest</option>
            <option value="old">Oldest</option>
            <option value="quick">Quickest read</option>
            <option value="top">Most popular</option>
            <option value="discussed">Most discussed</option>
            <option value="unread">Unread first</option>
          </Select>
        </Field>
        <Field label="Default content view">
          <Select value={form.content_view} onChange={(e) => patch({ content_view: e.target.value as Form["content_view"] })}>
            <option value="all">All</option>
            <option value="reading">Reading</option>
            <option value="video">Videos</option>
          </Select>
        </Field>
        <Field label="Page size">
          <Input
            type="number"
            min={1}
            max={100}
            value={form.page_size}
            onChange={(e) => patch({ page_size: Math.min(100, Math.max(1, Number(e.target.value) || 1)) })}
          />
        </Field>
        <Field label="Timezone (IANA)">
          <Input value={form.timezone} onChange={(e) => patch({ timezone: e.target.value })} placeholder="Europe/Warsaw" />
        </Field>
      </div>

      <label className="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          className="size-4 accent-primary"
          checked={form.auto_mark_read}
          onChange={(e) => patch({ auto_mark_read: e.target.checked })}
        />
        Mark items read when leaving the feed page
      </label>

      {/* Dirty-state save bar (§9.7). */}
      {dirty && (
        <div className="sticky bottom-4 flex items-center justify-between gap-3 rounded-md border border-border bg-card p-3 shadow-lg">
          <span className="text-sm text-muted-foreground">You have unsaved changes.</span>
          <div className="flex gap-2">
            <Button variant="outline" size="sm" onClick={reset} disabled={update.isPending}>
              Discard
            </Button>
            <Button size="sm" onClick={save} disabled={update.isPending}>
              {update.isPending ? "Saving…" : "Save"}
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="space-y-1.5">
      <Label>{label}</Label>
      {children}
    </div>
  );
}
