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
import type {
    AiProvider,
    AiSettings,
    TextProviderMode,
    VideoProviderMode,
} from "@/lib/types";

interface TextProviderRoutingProps {
    providers: AiProvider[];
    settings: AiSettings;
    route?: "text" | "video";
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
    route = "text",
}: TextProviderRoutingProps) {
    const update = useUpdateAiSettings();
    const textProviders = providers.filter((provider) =>
        route === "video"
            ? provider.provider_type === "gemini"
            : !provider.is_video_only,
    );
    const selectedIds =
        route === "video"
            ? settings.video_provider_ids.filter((id) =>
                  textProviders.some((provider) => provider.id === id),
              )
            : effectiveTextProviderIds(providers, settings.text_provider_ids);
    const mode =
        route === "video"
            ? settings.video_provider_mode
            : settings.text_provider_mode;
    const selectedProvider = selectedIds[0] ?? null;

    const save = (
        provider_mode: TextProviderMode | VideoProviderMode,
        provider_ids: number[],
    ) =>
        update.mutate(
            route === "video"
                ? {
                      video_provider_mode: provider_mode,
                      video_provider_ids: provider_ids,
                  }
                : {
                      text_provider_mode: provider_mode,
                      text_provider_ids: provider_ids,
                  },
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
                    {route === "video"
                        ? "YouTube video providers"
                        : "Text providers"}
                </h3>
                <Select
                    value={mode}
                    onValueChange={(value: TextProviderMode) =>
                        save(value, selectedIds.slice(0, 1))
                    }
                    disabled={update.isPending}
                >
                    <SelectTrigger
                        aria-label={
                            route === "video"
                                ? "YouTube video provider mode"
                                : "Text provider mode"
                        }
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

            {mode === "single" ? (
                <div className="max-w-xs space-y-1.5">
                    <Label htmlFor={`${route}-provider`}>
                        {route === "video" ? "Video provider" : "Text provider"}
                    </Label>
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
                        <SelectTrigger id={`${route}-provider`}>
                            <SelectValue placeholder="Choose a provider" />
                        </SelectTrigger>
                        <SelectContent>
                            <SelectItem value="none">
                                {route === "video"
                                    ? "Off - use transcripts"
                                    : "None"}
                            </SelectItem>
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
                        <Label htmlFor={`${route}-fallback-provider`}>
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
                            <SelectTrigger id={`${route}-fallback-provider`}>
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
