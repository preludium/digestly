import { useState } from "react";
import { toast } from "sonner";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { Field } from "@/components/common/SettingsTile";
import { Button } from "@/components/ui/button";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogHeader,
    DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
} from "@/components/ui/select";
import { useAiPresets, useCreateProvider } from "@/hooks/useAi";
import { apiError } from "@/lib/apiError";
import type { AiPreset, ApiStyle, NewAiProvider } from "@/lib/types";

const CUSTOM = "__custom__";

/** Add-provider modal (prompt.md §9.7 AI tab): pick a preset (endpoint+style baked in, key+model)
 *  or a custom endpoint (full fields). Keys are password inputs, submitted once, never rendered
 *  back (write-only, §6/§11). */
export function AddProviderModal({
    open,
    onOpenChange,
}: {
    open: boolean;
    onOpenChange: (o: boolean) => void;
}) {
    const presets = useAiPresets();
    return (
        <Dialog open={open} onOpenChange={onOpenChange}>
            <DialogContent className="bg-background">
                <DialogHeader>
                    <DialogTitle>Add an AI provider</DialogTitle>
                    <DialogDescription>
                        Pick a preset or configure a custom OpenAI or Anthropic
                        endpoint.
                    </DialogDescription>
                </DialogHeader>
                {presets.isError ? (
                    <ErrorBanner error={presets.error} />
                ) : (
                    <ProviderForm
                        presets={presets.data ?? []}
                        onDone={() => onOpenChange(false)}
                    />
                )}
            </DialogContent>
        </Dialog>
    );
}

function ProviderForm({
    presets,
    onDone,
}: {
    presets: AiPreset[];
    onDone: () => void;
}) {
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
            toast.error("Name, base URL and model are required");
            return;
        }
        const body: NewAiProvider = {
            name: name.trim(),
            provider_type: custom
                ? "custom"
                : (preset?.provider_type ?? "custom"),
            api_style: apiStyle,
            base_url: baseUrl.trim(),
            model: model.trim(),
            key: key.trim() || undefined,
        };
        create.mutate(body, {
            onSuccess: () => {
                toast.success("Provider added");
                onDone();
            },
            onError: (e) => toast.error(apiError(e, "Could not add provider")),
        });
    };

    return (
        <div className="space-y-4">
            <div className="space-y-1.5">
                <Label htmlFor="preset">Provider</Label>
                <Select value={choice} onValueChange={(v) => selectPreset(v)}>
                    <SelectTrigger id="preset">
                        <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                        {presets.map((p) => (
                            <SelectItem
                                key={p.provider_type}
                                value={p.provider_type}
                            >
                                {p.name}
                            </SelectItem>
                        ))}
                        <SelectItem value={CUSTOM}>Custom endpoint…</SelectItem>
                    </SelectContent>
                </Select>
            </div>

            <Field label="Provider name (account/project)">
                <Input
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    placeholder={custom ? "My LLM" : preset?.name}
                    autoFocus
                />
            </Field>

            {custom && (
                <>
                    <Field label="Base URL">
                        <Input
                            value={baseUrl}
                            onChange={(e) => setBaseUrl(e.target.value)}
                            placeholder="https://api.example.com/v1"
                        />
                    </Field>
                    <Field label="API style">
                        <Select
                            value={apiStyle}
                            onValueChange={(v) => setApiStyle(v as ApiStyle)}
                        >
                            <SelectTrigger>
                                <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                                <SelectItem value="openai">OpenAI</SelectItem>
                                <SelectItem value="anthropic">
                                    Anthropic
                                </SelectItem>
                            </SelectContent>
                        </Select>
                    </Field>
                </>
            )}

            {!custom && preset && (
                <div className="rounded-md border border-border bg-muted/40 p-3 text-xs text-muted-foreground">
                    <p>
                        Endpoint:{" "}
                        <span className="font-mono">{preset.base_url}</span>
                    </p>
                    <p>
                        API style:{" "}
                        {preset.api_style === "anthropic"
                            ? "Anthropic"
                            : "OpenAI"}
                    </p>
                </div>
            )}

            <Field label="Model">
                <Input
                    value={model}
                    onChange={(e) => setModel(e.target.value)}
                    placeholder="model-name"
                />
            </Field>

            <Field
                label={needsKey ? "API key" : "API key (optional for local)"}
            >
                <Input
                    type="password"
                    value={key}
                    onChange={(e) => setKey(e.target.value)}
                    placeholder={
                        needsKey
                            ? "sk-…"
                            : "leave blank for a keyless local server"
                    }
                    autoComplete="off"
                />
            </Field>
            <p className="text-xs text-muted-foreground">
                The key is stored encrypted and never shown again. To change it
                later, delete and re-add the provider.
            </p>

            <div className="flex justify-end gap-2 pt-2">
                <Button onClick={submit} disabled={create.isPending}>
                    {create.isPending ? "Adding…" : "Add provider"}
                </Button>
            </div>
        </div>
    );
}
