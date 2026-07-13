import type * as React from "react";
import { cn } from "@/lib/utils";

/** Loading placeholder (prompt.md §9 loading states). */
export function Skeleton({
    className,
    ...props
}: React.HTMLAttributes<HTMLDivElement>) {
    return (
        <div
            className={cn("animate-pulse rounded-md bg-muted", className)}
            {...props}
        />
    );
}
