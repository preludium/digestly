/** Shared error-toast unwrap: `Error` (incl. `ApiError`) surfaces its message, anything else
 *  falls back to a caller-supplied string. */
export function apiError(e: unknown, fallback: string): string {
    return e instanceof Error ? e.message : fallback;
}
