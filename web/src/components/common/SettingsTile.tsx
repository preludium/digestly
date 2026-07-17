import { Label } from "@/components/ui/label";
import { NumberField } from "@/components/ui/number-field";

/** Shared "tile" look for boxed settings rows - bordered card, title + description, control(s)
 *  on the right. Used by Notifications' "Notify me" switches, Connected accounts, AI providers,
 *  and Ingestion's toggle so every settings tab reads as one consistent system. */
export const SETTINGS_TILE_CLASS =
    "rounded-lg border border-border bg-card p-3.5";

export function TileTitle({
    title,
    description,
}: {
    title: React.ReactNode;
    description?: React.ReactNode;
}) {
    return (
        <span className="min-w-0">
            <span className="block text-sm font-semibold">{title}</span>
            {description && (
                <span className="mt-0.5 block text-xs text-muted-foreground">
                    {description}
                </span>
            )}
        </span>
    );
}

export function Field({
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

/** Labelled numeric setting with an optional hint underneath - the unit of every settings grid
 *  (AI generation params, ingestion, retention). */
export function NumField({
    label,
    description,
    value,
    onChange,
    min,
    max,
    step,
    unit,
}: {
    label: string;
    description?: string;
    value: number;
    onChange: (v: number) => void;
    min?: number;
    max?: number;
    step?: number;
    unit?: string;
}) {
    return (
        <Field label={label}>
            <NumberField
                value={value}
                onChange={onChange}
                min={min}
                max={max}
                step={step}
                unit={unit}
            />
            {description && (
                <p className="text-xs text-muted-foreground">{description}</p>
            )}
        </Field>
    );
}
