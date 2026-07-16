// Seed helpers + constants shared by every e2e spec. Specs seed their prerequisites through the
// real API (register, subscribe, ingest, admin digest run) and drive the UI only for the behavior
// they actually assert, per the approved spec (stage-2 "Test isolation & setup strategy").
//
// Cookie contract: `page.request` shares the browser context's cookie jar with `page`, so seeding
// the spec's own user via `page.request` leaves `page.goto()` already authenticated - no
// storageState needed. A second identity (e.g. admin) must use `request.newContext({ baseURL })`
// so its `hf_session` never clobbers the page user's.
import { type APIRequestContext, request } from "@playwright/test";
import { RSS_FEED_TITLE, RSS_ITEM_TITLE } from "./fixture-server.mjs";

export const APP_URL = "http://localhost:8099";

export const ADMIN = {
    username: "admin",
    password: process.env.E2E_ADMIN_PASSWORD ?? "e2e-admin-pw",
};

export const FIXTURE = {
    rss: "http://localhost:8098/rss.xml",
    atom: "http://localhost:8098/atom.xml",
    json: "http://localhost:8098/feed.json",
    rssFeedTitle: RSS_FEED_TITLE,
    rssItemTitle: RSS_ITEM_TITLE,
};

let usernameCounter = 0;

/** Unique per call → order-independent tests and dodges the 60s per-user ingest cooldown. */
export function uniqueUsername(prefix = "e2e"): string {
    usernameCounter += 1;
    return `${prefix}${Date.now().toString(36)}${usernameCounter}`;
}

async function ok(response: {
    ok(): boolean;
    status(): number;
    url(): string;
}) {
    if (!response.ok()) {
        throw new Error(
            `request to ${response.url()} failed with status ${response.status()}`,
        );
    }
}

/**
 * PUT /api/settings to mark the session's user onboarded. The first-run onboarding overlay
 * (App.tsx gates it on `!settings.onboarded`) is a full-screen `fixed inset-0` layer that
 * intercepts pointer events, so every UI-driving spec must clear it first. Marking it through the
 * API keeps the setup out of the UI and out of each spec's body.
 */
export async function markOnboarded(request: APIRequestContext): Promise<void> {
    const response = await request.put(`${APP_URL}/api/settings`, {
        data: { onboarded: true },
    });
    await ok(response);
}

/** POST /api/auth/register - auto-logs in and sets the `hf_session` cookie in the caller's jar. */
export async function registerUser(
    request: APIRequestContext,
    opts?: { username?: string; password?: string },
): Promise<{ username: string; password: string }> {
    const username = opts?.username ?? uniqueUsername();
    const password = opts?.password ?? "e2e-password-1";
    const response = await request.post(`${APP_URL}/api/auth/register`, {
        data: { username, password },
    });
    await ok(response);
    await markOnboarded(request);
    return { username, password };
}

/** POST /api/auth/login, then clear the onboarding overlay so the session can drive the UI. */
export async function loginAs(
    request: APIRequestContext,
    username: string,
    password: string,
): Promise<void> {
    const response = await request.post(`${APP_URL}/api/auth/login`, {
        data: { username, password },
    });
    await ok(response);
    await markOnboarded(request);
}

/** GET /api/categories, first id. Fresh users get default categories seeded on register. */
export async function firstCategoryId(
    request: APIRequestContext,
): Promise<number> {
    const response = await request.get(`${APP_URL}/api/categories`);
    await ok(response);
    const categories: Array<{ id: number }> = await response.json();
    if (categories.length === 0) {
        throw new Error("expected at least one default category");
    }
    return categories[0].id;
}

/** POST /api/feeds - direct subscribe (category is mandatory server-side). */
export async function subscribeFeed(
    request: APIRequestContext,
    feedUrl: string,
    opts?: { kind?: string; categoryId?: number },
): Promise<number> {
    const categoryId = opts?.categoryId ?? (await firstCategoryId(request));
    const response = await request.post(`${APP_URL}/api/feeds`, {
        data: {
            feed_url: feedUrl,
            kind: opts?.kind ?? "rss",
            category_id: categoryId,
        },
    });
    await ok(response);
    const feed: { id: number } = await response.json();
    return feed.id;
}

/** POST /api/feeds/refresh-all, then poll GET /api/ingest/status until the run clears. */
export async function ingestNow(
    request: APIRequestContext,
    opts?: { timeoutMs?: number },
): Promise<void> {
    const response = await request.post(`${APP_URL}/api/feeds/refresh-all`);
    await ok(response);

    const timeoutMs = opts?.timeoutMs ?? 30_000;
    const deadline = Date.now() + timeoutMs;
    for (;;) {
        const status = await request.get(`${APP_URL}/api/ingest/status`);
        await ok(status);
        const body: { run: unknown } = await status.json();
        if (body.run === null) {
            return;
        }
        if (Date.now() >= deadline) {
            throw new Error(`ingest run did not finish within ${timeoutMs}ms`);
        }
        await new Promise((resolve) => setTimeout(resolve, 250));
    }
}

/** GET /api/items, polling until `predicate` matches (default: at least one item). */
export async function waitForItems(
    request: APIRequestContext,
    opts?: {
        predicate?: (items: unknown[]) => boolean;
        timeoutMs?: number;
        pollMs?: number;
    },
): Promise<unknown[]> {
    const predicate =
        opts?.predicate ?? ((items: unknown[]) => items.length > 0);
    const timeoutMs = opts?.timeoutMs ?? 15_000;
    const pollMs = opts?.pollMs ?? 250;
    const deadline = Date.now() + timeoutMs;
    for (;;) {
        const response = await request.get(`${APP_URL}/api/items`);
        await ok(response);
        const page: { items: unknown[] } = await response.json();
        if (predicate(page.items)) {
            return page.items;
        }
        if (Date.now() >= deadline) {
            throw new Error(`no matching items within ${timeoutMs}ms`);
        }
        await new Promise((resolve) => setTimeout(resolve, pollMs));
    }
}

/** Subscribe the fixture RSS feed, ingest, and wait for its known item to appear. */
export async function seedFeedWithItems(
    request: APIRequestContext,
    feedUrl: string = FIXTURE.rss,
): Promise<{ feedId: number }> {
    const feedId = await subscribeFeed(request, feedUrl);
    await ingestNow(request);
    await waitForItems(request, {
        predicate: (items) =>
            items.some(
                (item) =>
                    (item as { title?: string }).title === FIXTURE.rssItemTitle,
            ),
    });
    return { feedId };
}

/**
 * Runs the admin digest for all users. Uses a SEPARATE request context so the admin's
 * `hf_session` never clobbers the calling spec's own logged-in user (see cookie contract above).
 */
export async function adminDigestRun(opts?: {
    baseURL?: string;
    lookbackDays?: number;
}): Promise<void> {
    const context = await request.newContext({
        baseURL: opts?.baseURL ?? APP_URL,
    });
    try {
        await loginAs(context, ADMIN.username, ADMIN.password);
        const response = await context.post("/api/digest/run", {
            data: opts?.lookbackDays
                ? { lookback_days: opts.lookbackDays }
                : undefined,
        });
        await ok(response);
    } finally {
        await context.dispose();
    }
}
