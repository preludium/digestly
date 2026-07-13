import { Minus, Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

interface NumberFieldProps {
    id?: string;
    value: number;
    onChange: (value: number) => void;
    min?: number;
    max?: number;
    step?: number;
    /** Static, non-editable unit shown inside the field next to the value (e.g. "days", "hrs"). */
    unit?: string;
    className?: string;
    "aria-label"?: string;
}

export function NumberField({
    id,
    value,
    onChange,
    min = 0,
    max = Infinity,
    step = 1,
    unit,
    className,
    ...aria
}: NumberFieldProps) {
    const clamp = (n: number) => Math.min(max, Math.max(min, n));
    return (
        <div className={cn("flex items-stretch", className)}>
            <Button
                type="button"
                variant="secondary"
                size="icon"
                className="h-10 rounded-r-none border-r-0"
                aria-label="Decrease"
                disabled={value <= min}
                onClick={() => onChange(clamp(value - step))}
            >
                <Minus className="size-4" />
            </Button>
            <div className="flex h-10 items-center gap-1 border border-input bg-popover px-2.5 text-sm">
                <input
                    id={id}
                    inputMode="numeric"
                    pattern="[0-9]*"
                    className="w-8 min-w-0 bg-transparent text-center outline-none [appearance:textfield]"
                    value={value}
                    onChange={(e) => {
                        const n = Number(e.target.value.replace(/\D/g, ""));
                        onChange(clamp(Number.isNaN(n) ? min : n));
                    }}
                    {...aria}
                />
                {unit && (
                    <span className="shrink-0 whitespace-nowrap text-xs text-muted-foreground">
                        {unit}
                    </span>
                )}
            </div>
            <Button
                type="button"
                variant="secondary"
                size="icon"
                className="h-10 rounded-l-none border-l-0"
                aria-label="Increase"
                disabled={value >= max}
                onClick={() => onChange(clamp(value + step))}
            >
                <Plus className="size-4" />
            </Button>
        </div>
    );
}
