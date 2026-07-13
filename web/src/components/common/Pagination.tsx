import { ChevronLeft, ChevronRight } from "lucide-react";
import { Button } from "@/components/ui/button";

/** Numbered prev/next pagination (prompt.md §9.1 - NOT infinite scroll). Reused by the feed
 *  and search grids. */
export function Pagination({
    page,
    totalPages,
    onPage,
}: {
    page: number;
    totalPages: number;
    onPage: (page: number) => void;
}) {
    if (totalPages <= 1) return null;

    // A small window of page numbers around the current page.
    const window = pageWindow(page, totalPages);

    return (
        <nav
            className="flex flex-wrap items-center justify-center gap-2"
            aria-label="Pagination"
        >
            <Button
                variant="outline"
                size="sm"
                disabled={page <= 1}
                onClick={() => onPage(page - 1)}
                aria-label="Previous page"
            >
                <ChevronLeft className="size-4" />
            </Button>

            {window.map((p, i) =>
                p === "…" ? (
                    <span
                        key={`gap-${i}`}
                        className="px-1 text-sm text-muted-foreground"
                    >
                        …
                    </span>
                ) : (
                    <Button
                        key={p}
                        variant={p === page ? "default" : "outline"}
                        size="sm"
                        aria-current={p === page ? "page" : undefined}
                        onClick={() => onPage(p)}
                    >
                        {p}
                    </Button>
                ),
            )}

            <Button
                variant="outline"
                size="sm"
                disabled={page >= totalPages}
                onClick={() => onPage(page + 1)}
                aria-label="Next page"
            >
                <ChevronRight className="size-4" />
            </Button>
        </nav>
    );
}

function pageWindow(page: number, total: number): (number | "…")[] {
    const out: (number | "…")[] = [];
    const push = (p: number) => out.push(p);
    const from = Math.max(1, page - 1);
    const to = Math.min(total, page + 1);

    push(1);
    if (from > 2) out.push("…");
    for (let p = from; p <= to; p++) if (p !== 1 && p !== total) push(p);
    if (to < total - 1) out.push("…");
    if (total > 1) push(total);
    return out;
}
