import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select } from "@/components/ui/select";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { useAiPresets, useCreateProvider } from "@/hooks/useAi";
import { toast } from "@/stores/toast";
import type { AiPreset, ApiStyle, NewAiProvider } from "@/lib/types";

const CUSTOM = "__custom__";

/** Add-provider modal (prompt.md §9.7 AI tab): pick a preset (endpoint+style baked in, key+model)
 *  or a custom endpoint (full fields). Keys are password inputs, submitted once, never rendered
 *  back (write-only, §6/§11). */
export function AddProviderModal({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const presets = useAiPresets();
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Add an AI provider</DialogTitle>
          <DialogDescription>Pick a preset or configure a custom OpenAI-compatible / Anthropic endpoint.</DialogDescription>
        </DialogHeader>
        {presets.isError ? (
          <ErrorBanner error={presets.error} />
        ) : (
          <ProviderForm presets={presets.data ?? []} onDone={() => onOpenChange(false)} />
        )}
      </DialogContent>
    </Dialog>
  );
}

function ProviderForm({ presets, onDone }: { presets: AiPreset[]; onDone: () => void }) {
  const create = useCreateProvider();
  const [choice, setChoice] = useState<string>(CUSTOM);
  const preset = presets.find((p) => p.provider_type === choice) ?? null;
  const custom = choice === CUSTOM;

  // Form fields (seeded from the chosen preset).
  const [name, setName] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiStyle, setApiStyle] = useState<ApiStyle>("openai");
  const [model, setModel] = useState("");
  const [key, setKey] = useState("");

  const selectPreset = (v: string) => {
    setChoice(v);
    const p = presets.find((x) => x.provider_type === v);
    if (p) {
      setName(p.name);
      setBaseUrl(p.base_url);
      setApiStyle(p.api_style);
      setModel(p.default_model);
    } else {
      setName("");
      setBaseUrl("");
      setApiStyle("openai");
      setModel("");
    }
    setKey("");
  };

  const needsKey = custom ? true : (preset?.needs_key ?? true);

  const submit = () => {
    if (!name.trim() || !baseUrl.trim() || !model.trim()) {
      toast("Name, base URL and model are required", "error");
      return;
    }
    const body: NewAiProvider = {
      name: name.trim(),
      provider_type: custom ? "custom" : (preset?.provider_type ?? "custom"),
      api_style: apiStyle,
      base_url: baseUrl.trim(),
      model: model.trim(),
      key: key.trim() || undefined,
    };
    create.mutate(body, {
      onSuccess: () => {
        toast("Provider added", "success");
        onDone();
      },
      onError: (e) => toast(e instanceof Error ? e.message : "Could not add provider", "error"),
    });
  };

  return (
    <div className="space-y-4">
      <div className="space-y-1.5">
        <Label htmlFor="preset">Provider</Label>
        <Select id="preset" value={choice} onChange={(e) => selectPreset(e.target.value)}>
          {presets.map((p) => (
            <option key={p.provider_type} value={p.provider_type}>
              {p.name}
            </option>
          ))}
          <option value={CUSTOM}>Custom endpoint…</option>
        </Select>
      </div>

      {custom && (
        <>
          <Field label="Name">
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="My LLM" autoFocus />
          </Field>
          <Field label="Base URL">
            <Input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="https://api.example.com/v1" />
          </Field>
          <Field label="API style">
            <Select value={apiStyle} onChange={(e) => setApiStyle(e.target.value as ApiStyle)}>
              <option value="openai">OpenAI-compatible</option>
              <option value="anthropic">Anthropic</option>
            </Select>
          </Field>
        </>
      )}

      {!custom && preset && (
        <div className="rounded-md border border-border bg-muted/40 p-3 text-xs text-muted-foreground">
          <p>
            Endpoint: <span className="font-mono">{preset.base_url}</span>
          </p>
          <p>API style: {preset.api_style === "anthropic" ? "Anthropic" : "OpenAI-compatible"}</p>
        </div>
      )}

      <Field label="Model">
        <Input value={model} onChange={(e) => setModel(e.target.value)} placeholder="model-name" />
      </Field>

      <Field label={needsKey ? "API key" : "API key (optional for local)"}>
        <Input
          type="password"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          placeholder={needsKey ? "sk-…" : "leave blank for a keyless local server"}
          autoComplete="off"
        />
      </Field>
      <p className="text-xs text-muted-foreground">
        The key is stored encrypted and never shown again. To change it later, delete and re-add the provider.
      </p>

      <div className="flex justify-end gap-2 pt-2">
        <Button onClick={submit} disabled={create.isPending}>
          {create.isPending ? "Adding…" : "Add provider"}
        </Button>
      </div>
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
