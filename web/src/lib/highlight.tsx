import type { ReactNode } from "react";

/** Escape a string for safe use inside a RegExp. */
// biome-ignore lint/suspicious/noShadowRestrictedNames: existing baseline
function escape(s: string): string {
    return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Wrap matched query terms in `<mark>` for search highlighting (§9.2). Terms are the query's
 * alphanumeric words; matching is case-insensitive. Returns the plain string when there's nothing
 * to highlight.
 */
export function highlight(
    text: string | null | undefined,
    query: string,
): ReactNode {
    const value = text ?? "";
    const terms = query
        .split(/\s+/)
        .map((t) => t.replace(/[^\p{L}\p{N}]/gu, ""))
        .filter(Boolean);
    if (!value || terms.length === 0) return value;

    // Capturing group → matched terms land on odd indices of the split result.
    const re = new RegExp(`(${terms.map(escape).join("|")})`, "gi");
    const parts = value.split(re);
    return parts.map((part, i) =>
        i % 2 === 1 ? (
            // biome-ignore lint/suspicious/noArrayIndexKey: existing baseline
            <mark key={i} className="rounded bg-primary/20 text-foreground">
                {part}
            </mark>
        ) : (
            part
        ),
    );
}
