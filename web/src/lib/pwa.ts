// PWA service-worker registration (prompt.md §9a). App-shell + offline reading, plus the offline
// write-sync trigger (S3): the SW's Background Sync wakes on reconnect and messages open clients to
// flush the outbox. Dispatches `hf-sw-update` when a new worker is waiting so the UI can offer a
// reload.

export function registerServiceWorker() {
    if (typeof navigator === "undefined" || !("serviceWorker" in navigator))
        return;
    // Dev is served by Vite (no SW); only register for the production build.
    if (!import.meta.env.PROD) return;

    window.addEventListener("load", () => {
        navigator.serviceWorker
            .register("/sw.js")
            .then((reg) => {
                reg.addEventListener("updatefound", () => {
                    const worker = reg.installing;
                    if (!worker) return;
                    worker.addEventListener("statechange", () => {
                        // A new SW installed while an old one controls the page → update available.
                        if (
                            worker.state === "installed" &&
                            navigator.serviceWorker.controller
                        ) {
                            window.dispatchEvent(
                                new CustomEvent("hf-sw-update"),
                            );
                        }
                    });
                });
            })
            .catch(() => {
                /* SW registration is best-effort; the app works without it. */
            });
    });
}

/** Ask the service worker to replay the outbox when connectivity returns, via the Background Sync
 *  API. Best-effort: unsupported browsers (Safari/Firefox) simply rely on the `online` event and
 *  the on-load flush in `useOutboxSync`. */
export function requestOutboxSync() {
    if (typeof navigator === "undefined" || !("serviceWorker" in navigator))
        return;
    navigator.serviceWorker.ready
        .then((reg) => {
            const sync = (
                reg as ServiceWorkerRegistration & {
                    sync?: { register(tag: string): Promise<void> };
                }
            ).sync;
            return sync?.register("hf-outbox");
        })
        .catch(() => {
            /* no Background Sync - the online-event fallback covers it */
        });
}

/** Tell the waiting worker to activate, then reload to pick up the new build. */
export function applyServiceWorkerUpdate() {
    navigator.serviceWorker?.getRegistration().then((reg) => {
        reg?.waiting?.postMessage({ type: "SKIP_WAITING" });
        // Reload once the new worker takes control.
        navigator.serviceWorker.addEventListener(
            "controllerchange",
            () => window.location.reload(),
            { once: true },
        );
    });
}
