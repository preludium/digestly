// Digestly service worker (prompt.md §9a): app-shell caching (installable, opens offline) + offline
// *reading* of already-fetched items/content. Non-GET and auth requests always go to the network;
// read/star writes made offline are queued in the app's outbox (S3) - this worker's Background Sync
// wakes on reconnect and messages open clients to replay that queue.

const VERSION = "digestly-v2";
const SHELL_CACHE = `${VERSION}-shell`;
const ASSET_CACHE = `${VERSION}-assets`;
const API_CACHE = `${VERSION}-api`;

// The app shell (SPA entry). Hashed assets are cached on first fetch (cache-first below).
const SHELL_URLS = ["/", "/index.html", "/manifest.webmanifest"];

self.addEventListener("install", (event) => {
    event.waitUntil(
        caches
            .open(SHELL_CACHE)
            .then((c) => c.addAll(SHELL_URLS))
            .catch(() => {}),
    );
});

self.addEventListener("activate", (event) => {
    event.waitUntil(
        (async () => {
            const keys = await caches.keys();
            await Promise.all(
                keys
                    .filter((k) => !k.startsWith(VERSION))
                    .map((k) => caches.delete(k)),
            );
            await self.clients.claim();
        })(),
    );
});

self.addEventListener("message", (event) => {
    if (event.data && event.data.type === "SKIP_WAITING") self.skipWaiting();
});

// Offline write-sync (S3): the app registers a `hf-outbox` sync when it queues a mutation offline.
// The browser fires this once connectivity returns (even if the app was closed and reopened),
// and we tell any open window to replay its outbox. Replay itself lives in the page, where the
// session cookie and the query cache are - the worker only nudges it.
self.addEventListener("sync", (event) => {
    if (event.tag === "hf-outbox") event.waitUntil(notifyClientsToFlush());
});

async function notifyClientsToFlush() {
    const clients = await self.clients.matchAll({
        includeUncontrolled: true,
        type: "window",
    });
    for (const client of clients)
        client.postMessage({ type: "hf-flush-outbox" });
}

// biome-ignore lint/complexity/noExcessiveLinesPerFunction: existing service-worker handler
self.addEventListener("fetch", (event) => {
    const req = event.request;
    if (req.method !== "GET") return; // never cache/replay mutations (auth, star, etc.)

    const url = new URL(req.url);
    if (url.origin !== self.location.origin) return;

    // SPA navigations: network-first, fall back to the cached shell so deep links open offline.
    if (req.mode === "navigate") {
        event.respondWith(
            fetch(req).catch(
                async () =>
                    (await caches.match(req)) ||
                    (await caches.match("/")) ||
                    Response.error(),
            ),
        );
        return;
    }

    // API reads: network-first (fresh when online), cache the response, serve cache offline.
    if (url.pathname.startsWith("/api/")) {
        // Health checks and auth state shouldn't be served stale.
        if (
            url.pathname === "/api/health" ||
            url.pathname.startsWith("/api/auth")
        )
            return;
        event.respondWith(
            (async () => {
                try {
                    const res = await fetch(req);
                    if (res.ok) {
                        const cache = await caches.open(API_CACHE);
                        cache.put(req, res.clone());
                    }
                    return res;
                } catch {
                    const cached = await caches.match(req);
                    if (cached) return cached;
                    return new Response(JSON.stringify({ error: "offline" }), {
                        status: 503,
                        headers: { "Content-Type": "application/json" },
                    });
                }
            })(),
        );
        return;
    }

    // Hashed build assets are immutable: cache-first.
    event.respondWith(
        (async () => {
            const cached = await caches.match(req);
            if (cached) return cached;
            try {
                const res = await fetch(req);
                if (res.ok) {
                    const cache = await caches.open(ASSET_CACHE);
                    cache.put(req, res.clone());
                }
                return res;
            } catch {
                return cached || Response.error();
            }
        })(),
    );
});
