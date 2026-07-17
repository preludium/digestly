import { ArrowDown, ArrowUp } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
} from "@/components/ui/select";
import { useUpdateAiSettings } from "@/hooks/useAi";
import { apiError } from "@/lib/apiError";
import type { AiProvider, AiSettings, TextProviderMode } from "@/lib/types";

interface TextProviderRoutingProps {
    providers: AiProvider[];
    settings: AiSettings;
}

export function effectiveTextProviderIds(
    providers: AiProvider[],
    ids: number[],
): number[] {
    return ids.filter((id) =>
        providers.some(
            (provider) => provider.id === id && !provider.is_video_only,
        ),
    );
}

export function TextProviderRouting({
    providers,
    settings,
}: TextProviderRoutingProps) {
    const update = useUpdateAiSettings();
    const textProviders = providers.filter(
        (provider) => !provider.is_video_only,
    );
    const selectedIds = effectiveTextProviderIds(
        providers,
        settings.text_provider_ids,
    );
    const selectedProvider = selectedIds[0] ?? null;

    const save = (
        text_provider_mode: TextProviderMode,
        text_provider_ids: number[],
    ) =>
        update.mutate(
            { text_provider_mode, text_provider_ids },
            {
                onError: (error) =>
                    toast.error(apiError(error, "Could not save")),
            },
        );

    const move = (index: number, direction: -1 | 1) => {
        const nextIndex = index + direction;
        if (nextIndex < 0 || nextIndex >= selectedIds.length) return;
        const next = [...selectedIds];
        [next[index], next[nextIndex]] = [next[nextIndex], next[index]];
        save("ordered", next);
    };

    return (
        <div className="space-y-3.5">
            <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border pb-2">
                <h3 className="text-[13px] font-bold tracking-wide">
                    Text providers
                </h3>
                <Select
                    value={settings.text_provider_mode}
                    onValueChange={(value: TextProviderMode) =>
                        save(value, selectedIds.slice(0, 1))
                    }
                    disabled={update.isPending}
                >
                    <SelectTrigger
                        aria-label="Text provider mode"
                        className="w-44"
                    >
                        <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                        <SelectItem value="single">Single provider</SelectItem>
                        <SelectItem value="ordered">
                            Ordered fallback
                        </SelectItem>
                    </SelectContent>
                </Select>
            </div>

            {settings.text_provider_mode === "single" ? (
                <div className="max-w-xs space-y-1.5">
                    <Label htmlFor="text-provider">Text provider</Label>
                    <Select
                        value={
                            selectedProvider === null
                                ? "none"
                                : String(selectedProvider)
                        }
                        onValueChange={(value) =>
                            save(
                                "single",
                                value === "none" ? [] : [Number(value)],
                            )
                        }
                        disabled={update.isPending}
                    >
                        <SelectTrigger id="text-provider">
                            <SelectValue placeholder="Choose a provider" />
                        </SelectTrigger>
                        <SelectContent>
                            <SelectItem value="none">None</SelectItem>
                            {textProviders.map((provider) => (
                                <SelectItem
                                    key={provider.id}
                                    value={String(provider.id)}
                                >
                                    {provider.name} ({provider.model})
                                </SelectItem>
                            ))}
                        </SelectContent>
                    </Select>
                </div>
            ) : (
                <div className="space-y-2">
                    {selectedIds.map((id, index) => {
                        const provider = textProviders.find(
                            (item) => item.id === id,
                        );
                        if (!provider) return null;
                        return (
                            <div
                                key={provider.id}
                                className="flex items-center gap-3 rounded-md border border-border bg-card px-3 py-2"
                            >
                                <span className="min-w-0 flex-1 truncate text-sm font-medium">
                                    {provider.name} ({provider.model})
                                </span>
                                <Button
                                    variant="ghost"
                                    size="icon"
                                    aria-label={`Move ${provider.name} up`}
                                    disabled={update.isPending || index === 0}
                                    onClick={() => move(index, -1)}
                                >
                                    <ArrowUp className="size-4" />
                                </Button>
                                <Button
                                    variant="ghost"
                                    size="icon"
                                    aria-label={`Move ${provider.name} down`}
                                    disabled={
                                        update.isPending ||
                                        index === selectedIds.length - 1
                                    }
                                    onClick={() => move(index, 1)}
                                >
                                    <ArrowDown className="size-4" />
                                </Button>
                            </div>
                        );
                    })}
                    <div className="max-w-xs space-y-1.5">
                        <Label htmlFor="fallback-provider">
                            Add fallback provider
                        </Label>
                        <Select
                            value="none"
                            onValueChange={(value) => {
                                if (value !== "none")
                                    save("ordered", [
                                        ...selectedIds,
                                        Number(value),
                                    ]);
                            }}
                            disabled={update.isPending}
                        >
                            <SelectTrigger id="fallback-provider">
                                <SelectValue placeholder="Choose a provider" />
                            </SelectTrigger>
                            <SelectContent>
                                <SelectItem value="none">
                                    Choose a provider
                                </SelectItem>
                                {textProviders
                                    .filter(
                                        (provider) =>
                                            !selectedIds.includes(provider.id),
                                    )
                                    .map((provider) => (
                                        <SelectItem
                                            key={provider.id}
                                            value={String(provider.id)}
                                        >
                                            {provider.name} ({provider.model})
                                        </SelectItem>
                                    ))}
                            </SelectContent>
                        </Select>
                    </div>
                </div>
            )}
        </div>
    );
}
