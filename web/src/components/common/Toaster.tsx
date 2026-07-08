import { cn } from "@/lib/utils";
import { useToastStore } from "@/stores/toast";

/** Renders active toasts (prompt.md §9.14). Mounted once at the app root. */
export function Toaster() {
  const toasts = useToastStore((s) => s.toasts);
  const dismiss = useToastStore((s) => s.dismiss);

  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-0 z-[100] flex flex-col items-center gap-2 p-4 sm:items-end">
      {toasts.map((t) => (
        <button
          key={t.id}
          onClick={() => dismiss(t.id)}
          className={cn(
            "pointer-events-auto w-full max-w-sm rounded-md border px-4 py-3 text-left text-sm shadow-lg",
            t.variant === "error" && "border-destructive/50 bg-destructive/10 text-destructive",
            t.variant === "success" && "border-accent/50 bg-accent/10 text-foreground",
            t.variant === "default" && "border-border bg-card text-card-foreground",
          )}
        >
          {t.message}
        </button>
      ))}
    </div>
  );
}
