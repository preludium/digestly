import { useEffect, useLayoutEffect, useRef } from "react";

/** Debounced autosave: `delayMs` after `value` stops changing, calls `save(value)` - but only
 *  when it actually differs from the last value that was saved or loaded from the server. This
 *  skips the initial hydrate-from-server (not a user edit) and avoids re-saving the same data
 *  when a refetch after our own save echoes it back unchanged. Every settings tab persists this
 *  way instead of a manual Save button. */
export function useAutosave<T>(
    value: T | null | undefined,
    save: (value: T) => void,
    delayMs = 700,
) {
    const lastSynced = useRef<string | null>(null);

    // Kept in a ref so a caller can pass an inline closure without either restarting the debounce
    // on every render or firing a `save` that closed over stale state.
    const saveRef = useRef(save);
    useLayoutEffect(() => {
        saveRef.current = save;
    });

    useEffect(() => {
        if (value == null) return;
        const serialized = JSON.stringify(value);

        if (lastSynced.current === null) {
            lastSynced.current = serialized; // first value seen - server hydrate, not a user edit
            return;
        }
        if (serialized === lastSynced.current) return;

        const id = setTimeout(() => {
            lastSynced.current = serialized;
            saveRef.current(value);
        }, delayMs);
        return () => clearTimeout(id);
    }, [value, delayMs]);
}
