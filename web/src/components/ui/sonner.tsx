import {
    CircleCheckIcon,
    InfoIcon,
    Loader2Icon,
    OctagonXIcon,
    TriangleAlertIcon,
} from "lucide-react";
import { Toaster as Sonner, type ToasterProps } from "sonner";
import { useUiStore } from "@/stores/ui";

/** App-wide toast surface (§9.14). Mounted once at the root.
 *
 *  Diverges from the stock shadcn component in two ways: the theme comes from our own `ui` store
 *  (shadcn's version reaches for next-themes, which this app doesn't use), and the colors are
 *  wrapped in `hsl()` because our design tokens are bare HSL triplets, not full color values. */
const Toaster = ({ ...props }: ToasterProps) => {
    const theme = useUiStore((s) => s.theme);

    return (
        <Sonner
            theme={theme}
            className="toaster group"
            position="bottom-right"
            closeButton
            richColors
            icons={{
                success: <CircleCheckIcon className="size-4" />,
                info: <InfoIcon className="size-4" />,
                warning: <TriangleAlertIcon className="size-4" />,
                error: <OctagonXIcon className="size-4" />,
                loading: <Loader2Icon className="size-4 animate-spin" />,
            }}
            style={
                {
                    "--normal-bg": "hsl(var(--popover))",
                    "--normal-text": "hsl(var(--popover-foreground))",
                    "--normal-border": "hsl(var(--border))",
                    "--success-bg": "hsl(var(--card))",
                    "--success-text": "hsl(var(--success))",
                    "--success-border": "hsl(var(--success) / 0.35)",
                    "--error-bg": "hsl(var(--card))",
                    "--error-text": "hsl(var(--destructive))",
                    "--error-border": "hsl(var(--destructive) / 0.35)",
                    "--border-radius": "var(--radius)",
                } as React.CSSProperties
            }
            {...props}
        />
    );
};

export { Toaster };
