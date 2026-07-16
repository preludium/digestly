// biome-ignore lint/style/noExcessiveLinesPerFile: existing baseline
import {
    Pencil,
    Plus,
    Sparkles,
    Trash2,
    TriangleAlert,
    Wifi,
    Zap,
} from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { EmptyState } from "@/components/common/EmptyState";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { NameDialog } from "@/components/common/NameDialog";
import { AddProviderModal } from "@/components/settings/AddProviderModal";
import {
    NumField,
    SETTINGS_TILE_CLASS,
} from "@/components/settings/SettingsTile";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
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
import {
    useActivateProvider,
    useAiProviders,
    useAiSettings,
    useDeleteProvider,
    usePatchProvider,
    useSetVideoProvider,
    useTestProvider,
    useUpdateAiSettings,
} from "@/hooks/useAi";
import { useAutosave } from "@/hooks/useAutosave";
import type { AiProvider } from "@/lib/types";
import { cn } from "@/lib/utils";

/** Admin AI tab (prompt.md §9.7): provider manager (active radio · test · delete · key hidden) +
 *  global generation params. Admin-only; the server also enforces the role (§11). */
export function AiSettings() {
    const providers = useAiProviders();
    const [addOpen, setAddOpen] = useState(false);

    return (
        <div className="space-y-8">
            <div className="space-y-3.5">
                <div className="flex items-center justify-between gap-3 border-b border-border pb-2">
                    <h3 className="text-[13px] font-bold tracking-wide">
                        AI providers
                    </h3>
                    <Button size="sm" onClick={() => setAddOpen(true)}>
                        <Plus className="size-4" /> Add provider
                    </Button>
                </div>
                <p className="text-[13px] text-muted-foreground">
                    One provider is active instance-wide; every user's summaries
                    use it.
                </p>

                {providers.isLoading ? (
                    <div className="flex justify-center py-6">
                        <Spinner className="size-6" />
                    </div>
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
            </div>

            <VideoProviderPicker />

            <GlobalParams />

            <AddProviderModal open={addOpen} onOpenChange={setAddOpen} />
        </div>
    );
}

/** YouTube video summaries via a dedicated Gemini provider (prompt.md §6a video path): Gemini is
 *  sent the video URL directly - no transcript needed. Off (default) = the transcript flow. */
function VideoProviderPicker() {
    const providers = useAiProviders();
    const settings = useAiSettings();
    const setVideoProvider = useSetVideoProvider();

    const geminiProviders = (providers.data ?? []).filter(
        (p) => p.provider_type === "gemini",
    );
    const current = settings.data?.video_provider_id ?? null;

    const onChange = (value: string) => {
        const id = value === "off" ? null : Number(value);
        setVideoProvider.mutate(id, {
            onSuccess: () =>
                toast(
                    id === null
                        ? "Video summaries use transcripts again"
                        : "Video summaries now go to Gemini",
                ),
            onError: (e) =>
                toast.error(e instanceof Error ? e.message : "Could not save"),
        });
    };

    if (providers.isLoading || settings.isLoading) return null;

    return (
        <div className="space-y-3.5">
            <div className="border-b border-border pb-2">
                <h3 className="text-[13px] font-bold tracking-wide">
                    YouTube video summaries
                </h3>
            </div>
            <p className="text-[13px] text-muted-foreground">
                Summarize videos by sending Gemini the video URL directly, with
                no transcript fetch - so videos without captions still get
                summarized. Falls back to the transcript flow if the call fails.
            </p>
            {current !== null && (
                <Alert variant="warning" className="text-[13px]">
                    <TriangleAlert className="size-4" />
                    Video is billed by runtime - roughly 100 tokens per second,
                    so a 10-minute video costs about 60,000 tokens. The budgets
                    below fill far faster than they do for articles.
                </Alert>
            )}
            {providers.isError || settings.isError ? (
                <ErrorBanner error={providers.error ?? settings.error} />
            ) : geminiProviders.length === 0 ? (
                <p className="text-[13px] text-muted-foreground">
                    Add a <span className="font-medium">Google Gemini</span>{" "}
                    provider above to enable this.
                </p>
            ) : (
                <div className="max-w-xs space-y-1.5">
                    <Label htmlFor="video-provider">Video provider</Label>
                    <Select
                        value={current === null ? "off" : String(current)}
                        onValueChange={onChange}
                        disabled={setVideoProvider.isPending}
                    >
                        <SelectTrigger id="video-provider">
                            <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                            <SelectItem value="off">
                                Off - use transcripts
                            </SelectItem>
                            {geminiProviders.map((p) => (
                                <SelectItem key={p.id} value={String(p.id)}>
                                    {p.name} ({p.model})
                                </SelectItem>
                            ))}
                        </SelectContent>
                    </Select>
                </div>
            )}
        </div>
    );
}

// biome-ignore lint/complexity/noExcessiveLinesPerFunction: existing baseline
function ProviderRow({ provider }: { provider: AiProvider }) {
    const activate = useActivateProvider();
    const remove = useDeleteProvider();
    const patch = usePatchProvider();
    const test = useTestProvider();
    const [confirmDelete, setConfirmDelete] = useState(false);
    const [editModel, setEditModel] = useState(false);

    const saveModel = (model: string) => {
        patch.mutate(
            { id: provider.id, model },
            {
                onSuccess: () => toast("Model updated"),
                onError: (e) =>
                    toast.error(
                        e instanceof Error ? e.message : "Could not save",
                    ),
            },
        );
    };

    const runTest = () =>
        test.mutate(provider.id, {
            onSuccess: (r) =>
                r.ok
                    ? toast.success("Connection OK")
                    : toast.error(`Test failed: ${r.error ?? "unknown error"}`),
            onError: (e) =>
                toast.error(e instanceof Error ? e.message : "Test failed"),
        });

    const del = () => {
        remove.mutate(provider.id, {
            onSuccess: () => toast("Provider deleted"),
            onError: (e) =>
                toast.error(
                    e instanceof Error ? e.message : "Could not delete",
                ),
        });
    };

    return (
        <>
            <li
                className={cn(
                    SETTINGS_TILE_CLASS,
                    "flex flex-col gap-3 wide:flex-row wide:items-center",
                )}
            >
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
                            {provider.is_active && (
                                <Badge variant="success">active</Badge>
                            )}
                        </span>
                        <span className="mt-1.5 flex flex-wrap items-center gap-1.5">
                            <span className="inline-flex items-center whitespace-nowrap rounded-md bg-muted px-2 py-0.5 text-[11px] font-semibold text-muted-foreground">
                                {provider.provider_type}
                            </span>
                            <span className="inline-flex items-center whitespace-nowrap rounded-md bg-muted px-2 py-0.5 text-[11px] font-semibold text-muted-foreground">
                                {provider.api_style === "anthropic"
                                    ? "Anthropic"
                                    : "OpenAI"}
                            </span>
                            <span className="inline-flex items-center whitespace-nowrap rounded-md bg-muted px-2 py-0.5 text-[11px] font-semibold text-muted-foreground">
                                {provider.model}
                            </span>
                        </span>
                    </span>
                </label>

                <div className="flex items-center gap-2 wide:ml-auto">
                    <Button
                        variant="outline"
                        size="sm"
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
                    <Button
                        variant="ghost"
                        size="icon"
                        aria-label="Change model"
                        disabled={patch.isPending}
                        onClick={() => setEditModel(true)}
                    >
                        <Pencil className="size-4" />
                    </Button>
                    <Button
                        variant="ghost"
                        size="icon"
                        aria-label="Delete provider"
                        disabled={remove.isPending}
                        onClick={() => setConfirmDelete(true)}
                    >
                        <Trash2 className="size-4" />
                    </Button>
                </div>
            </li>

            <NameDialog
                open={editModel}
                onOpenChange={setEditModel}
                title="Change model"
                label="Model"
                initialValue={provider.model}
                placeholder="e.g. gemini-3.5-flash"
                onSubmit={saveModel}
            />

            <ConfirmDialog
                open={confirmDelete}
                onOpenChange={setConfirmDelete}
                title="Delete provider?"
                description={`Delete "${provider.name}"? To rotate a key you delete and re-add.`}
                confirmLabel="Delete"
                destructive
                onConfirm={del}
            />
        </>
    );
}

// biome-ignore lint/complexity/noExcessiveLinesPerFunction: existing baseline
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
            const {
                max_tokens,
                temperature,
                timeout_secs,
                daily_token_budget,
                monthly_token_budget,
            } = settings.data;
            setForm({
                max_tokens,
                temperature,
                timeout_secs,
                daily_token_budget,
                monthly_token_budget,
            });
        }
    }, [settings.data]);

    useAutosave(form, (f) =>
        update.mutate(f, {
            onError: (e) =>
                toast.error(e instanceof Error ? e.message : "Could not save"),
        }),
    );

    if (settings.isLoading)
        return (
            <div className="flex justify-center py-6">
                <Spinner className="size-6" />
            </div>
        );
    if (settings.isError) return <ErrorBanner error={settings.error} />;

    return (
        <div className="space-y-3.5">
            <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border pb-2">
                <h3 className="text-[13px] font-bold tracking-wide">
                    Generation settings
                </h3>
                <div className="flex flex-wrap items-center gap-2">
                    <TokenStat
                        label="Today"
                        value={settings.data?.tokens_used_today ?? 0}
                    />
                    <TokenStat
                        label="This month"
                        value={settings.data?.tokens_used_month ?? 0}
                    />
                </div>
            </div>

            <div className="grid gap-4 sm:grid-cols-2">
                <NumField
                    label="Max tokens"
                    value={form.max_tokens}
                    onChange={(v) => setForm((f) => ({ ...f, max_tokens: v }))}
                    min={64}
                    unit="tokens"
                />
                <NumField
                    label="Temperature"
                    step={0.1}
                    value={form.temperature}
                    onChange={(v) => setForm((f) => ({ ...f, temperature: v }))}
                    min={0}
                    max={2}
                />
                <NumField
                    label="Timeout"
                    value={form.timeout_secs}
                    onChange={(v) =>
                        setForm((f) => ({ ...f, timeout_secs: v }))
                    }
                    min={5}
                    unit="sec"
                />
                <NumField
                    label="Daily token budget"
                    description="0 = unlimited."
                    value={form.daily_token_budget}
                    onChange={(v) =>
                        setForm((f) => ({ ...f, daily_token_budget: v }))
                    }
                    min={0}
                    unit="tokens"
                />
                <NumField
                    label="Monthly token budget"
                    description="0 = unlimited."
                    value={form.monthly_token_budget}
                    onChange={(v) =>
                        setForm((f) => ({ ...f, monthly_token_budget: v }))
                    }
                    min={0}
                    unit="tokens"
                />
            </div>
        </div>
    );
}

function TokenStat({ label, value }: { label: string; value: number }) {
    return (
        <span className="inline-flex items-center gap-2 rounded-md border border-border bg-card px-2.5 py-1.5">
            <Zap className="size-3.5 shrink-0 text-muted-foreground" />
            <span className="text-xs text-muted-foreground">{label}</span>
            <span className="text-sm font-bold text-foreground">
                {value.toLocaleString()}
            </span>
        </span>
    );
}
