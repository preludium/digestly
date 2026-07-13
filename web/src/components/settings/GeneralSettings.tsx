import { useEffect, useState } from "react";
import { toast } from "sonner";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { NumberField } from "@/components/ui/number-field";
import {
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
} from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { useAutosave } from "@/hooks/useAutosave";
import { useSettings, useUpdateSettings } from "@/hooks/useSettings";
import type { UserSettings } from "@/lib/types";
import { useUiStore } from "@/stores/ui";

type Form = Omit<UserSettings, "onboarded">;

/** General preferences tab (prompt.md §9.7) - autosaves on every change (debounced). Theme +
 *  density also update the local UI store immediately for a live preview. */
export function GeneralSettings() {
    const settings = useSettings();
    const update = useUpdateSettings();
    const setDensity = useUiStore((s) => s.setDensity);

    const [form, setForm] = useState<Form | null>(null);

    useEffect(() => {
        if (settings.data) {
            const { onboarded: _o, ...rest } = settings.data;
            setForm(rest);
        }
    }, [settings.data]);

    useAutosave(form, (f) =>
        update.mutate(f, {
            onError: (e) =>
                toast.error(e instanceof Error ? e.message : "Could not save"),
        }),
    );

    if (settings.isLoading || !form)
        return (
            <div className="flex justify-center py-6">
                <Spinner className="size-6" />
            </div>
        );
    if (settings.isError) return <ErrorBanner error={settings.error} />;

    const patch = (p: Partial<Form>) =>
        setForm((f) => (f ? { ...f, ...p } : f));

    return (
        <div className="space-y-5">
            <div className="space-y-3.5">
                <h3 className="border-b border-border pb-2 text-[13px] font-bold tracking-wide">
                    Appearance
                </h3>
                <div className="grid gap-4 sm:grid-cols-2">
                    <Field label="List density">
                        <Select
                            value={form.density}
                            onValueChange={(v) => {
                                const val = v as Form["density"];
                                patch({ density: val });
                                setDensity(val);
                            }}
                        >
                            <SelectTrigger>
                                <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                                <SelectItem value="normal">Normal</SelectItem>
                                <SelectItem value="compact">Compact</SelectItem>
                            </SelectContent>
                        </Select>
                    </Field>
                    <Field label="Default content view">
                        <Select
                            value={form.content_view}
                            onValueChange={(v) =>
                                patch({
                                    content_view: v as Form["content_view"],
                                })
                            }
                        >
                            <SelectTrigger>
                                <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                                <SelectItem value="all">All</SelectItem>
                                <SelectItem value="reading">Reading</SelectItem>
                                <SelectItem value="video">Videos</SelectItem>
                            </SelectContent>
                        </Select>
                    </Field>
                </div>
            </div>

            <div className="space-y-3.5">
                <h3 className="border-b border-border pb-2 text-[13px] font-bold tracking-wide">
                    Reading
                </h3>
                <div className="grid gap-4 sm:grid-cols-2">
                    <Field label="Default sort">
                        <Select
                            value={form.sort}
                            onValueChange={(v) =>
                                patch({ sort: v as Form["sort"] })
                            }
                        >
                            <SelectTrigger>
                                <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                                <SelectItem value="new">Newest</SelectItem>
                                <SelectItem value="old">Oldest</SelectItem>
                                <SelectItem value="quick">
                                    Quickest read
                                </SelectItem>
                                <SelectItem value="top">
                                    Most popular
                                </SelectItem>
                                <SelectItem value="discussed">
                                    Most discussed
                                </SelectItem>
                                <SelectItem value="unread">
                                    Unread first
                                </SelectItem>
                            </SelectContent>
                        </Select>
                    </Field>
                    <Field label="Page size">
                        <NumberField
                            value={form.page_size}
                            onChange={(v) => patch({ page_size: v })}
                            min={1}
                            max={100}
                        />
                    </Field>
                </div>
                <label className="flex items-center gap-2 text-sm">
                    <Switch
                        checked={form.auto_mark_read}
                        onCheckedChange={(v) => patch({ auto_mark_read: v })}
                    />
                    Mark items read when leaving the feed page
                </label>
            </div>

            <div className="space-y-3.5">
                <h3 className="border-b border-border pb-2 text-[13px] font-bold tracking-wide">
                    Regional
                </h3>
                <div className="grid gap-4 sm:grid-cols-2">
                    <Field label="Timezone">
                        <Input
                            value={form.timezone}
                            onChange={(e) =>
                                patch({ timezone: e.target.value })
                            }
                            placeholder="Europe/Warsaw"
                        />
                    </Field>
                </div>
            </div>
        </div>
    );
}

function Field({
    label,
    children,
}: {
    label: string;
    children: React.ReactNode;
}) {
    return (
        <div className="space-y-1.5">
            <Label>{label}</Label>
            {children}
        </div>
    );
}
