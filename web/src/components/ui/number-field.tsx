import { Minus, Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

interface NumberFieldProps {
  id?: string;
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
  className?: string;
  "aria-label"?: string;
}

export function NumberField({ id, value, onChange, min = 0, max = Infinity, step = 1, className, ...aria }: NumberFieldProps) {
  const clamp = (n: number) => Math.min(max, Math.max(min, n));
  return (
    <div className={cn("flex items-stretch", className)}>
      <Button
        type="button"
        variant="outline"
        size="icon"
        className="h-10 rounded-r-none border-r-0"
        aria-label="Decrease"
        disabled={value <= min}
        onClick={() => onChange(clamp(value - step))}
      >
        <Minus className="size-4" />
      </Button>
      <Input
        id={id}
        inputMode="numeric"
        pattern="[0-9]*"
        className="h-10 w-16 rounded-none text-center [appearance:textfield]"
        value={value}
        onChange={(e) => {
          const n = Number(e.target.value.replace(/\D/g, ""));
          onChange(clamp(Number.isNaN(n) ? min : n));
        }}
        {...aria}
      />
      <Button
        type="button"
        variant="outline"
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
