import { Skeleton } from "@/components/ui/skeleton";

/** Card-grid loading skeleton, shared by the feed and search grids (§9.1, §9.2). */
export function CardGridSkeleton({ count = 8 }: { count?: number }) {
    return (
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 wide:grid-cols-3 xl:grid-cols-4">
            {Array.from({ length: count }).map((_, i) => (
                <div
                    key={i}
                    className="space-y-3 rounded-lg border border-border p-3"
                >
                    <Skeleton className="aspect-video w-full rounded-md" />
                    <Skeleton className="h-4 w-3/4" />
                    <Skeleton className="h-3 w-1/2" />
                    <Skeleton className="h-3 w-full" />
                </div>
            ))}
        </div>
    );
}
