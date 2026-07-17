import type { FeedStatus } from "@/lib/types";

export const STATUS_BADGE: Record<
    FeedStatus,
    { label: string; variant: "success" | "warning" | "destructive" }
> = {
    ok: { label: "Ok", variant: "success" },
    failing: { label: "Failing", variant: "warning" },
    disabled: { label: "Disabled", variant: "destructive" },
};
