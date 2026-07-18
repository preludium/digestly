// Seed helpers + constants shared by every e2e spec. Specs seed their prerequisites through the
// real API (register, subscribe, ingest, admin digest run) and drive the UI only for the behavior
// they actually assert, per the approved spec (stage-2 "Test isolation & setup strategy").
//
// Cookie contract: `page.request` shares the browser context's cookie jar with `page`, so seeding
// the spec's own user via `page.request` leaves `page.goto()` already authenticated - no
// storageState needed. A second identity (e.g. admin) must use `request.newContext({ baseURL })`
// so its `hf_session` never clobbers the page user's.
import { type APIRequestContext, request } from "@playwright/test";
import {
    RSS_FEED_TITLE,
    RSS_ITEM_TITLE,
    SUMMARY_ITEM_TITLE,
    YOUTUBE_ITEM_TITLE,
} from "./fixture-server.mjs";

export const APP_URL = "http://localhost:8099";

export const ADMIN = {
    username: "admin",
    password: process.env.E2E_ADMIN_PASSWORD ?? "e2e-admin-pw",
};

export const FIXTURE = {
    rss: "http://localhost:8098/rss.xml",
    atom: "http://localhost:8098/atom.xml",
    json: "http://localhost:8098/feed.json",
    summary: "http://localhost:8098/summary.json",
    youtube: "http://localhost:8098/youtube.xml",
    rssFeedTitle: RSS_FEED_TITLE,
    rssItemTitle: RSS_ITEM_TITLE,
    summaryItemTitle: SUMMARY_ITEM_TITLE,
    aiBaseUrl: "http://localhost:8098/ai-mock/openai",
};

export type AiMockResponse = {
    status: number;
    text?: string;
    error?: string;
};

export type AiProviderInput = {
    name: string;
    provider_type: string;
    model: string;
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

/** A separate admin session keeps the browser user's session untouched during e2e setup. */
export async function withAdmin<T>(
    action: (admin: APIRequestContext) => Promise<T>,
): Promise<T> {
    const admin = await request.newContext({ baseURL: APP_URL });
    try {
        await loginAs(admin, ADMIN.username, ADMIN.password);
        return await action(admin);
    } finally {
        await admin.dispose();
    }
}

/** Configure the fixture server's local OpenAI/Gemini responses and clear its request log. */
export async function configureAiMock(
    request: APIRequestContext,
    responses: Record<string, AiMockResponse>,
): Promise<void> {
    const response = await request.post(
        "http://localhost:8098/ai-mock/config",
        {
            data: { responses },
        },
    );
    await ok(response);
}

export async function aiMockRequests(
    request: APIRequestContext,
): Promise<Array<{ kind: string; model: string }>> {
    const response = await request.get(
        "http://localhost:8098/ai-mock/requests",
    );
    await ok(response);
    return (
        (await response.json()) as {
            requests: Array<{ kind: string; model: string }>;
        }
    ).requests;
}

/** Create an AI provider through the real admin endpoint, pointing it at the local fixture. */
export async function createFixtureAiProvider(
    request: APIRequestContext,
    input: AiProviderInput,
): Promise<number> {
    const response = await request.post(`${APP_URL}/api/ai/providers`, {
        data: {
            ...input,
            api_style: "openai",
            base_url: FIXTURE.aiBaseUrl,
            key: "e2e-fixture-key",
        },
    });
    await ok(response);
    return ((await response.json()) as { id: number }).id;
}

export async function deleteAiProvider(
    request: APIRequestContext,
    providerId: number,
): Promise<void> {
    const response = await request.delete(
        `${APP_URL}/api/ai/providers/${providerId}`,
    );
    await ok(response);
}

export async function resetAiMock(request: APIRequestContext): Promise<void> {
    const response = await request.post("http://localhost:8098/ai-mock/reset");
    await ok(response);
}

export async function aiSettings(request: APIRequestContext): Promise<{
    text_provider_mode: string;
    text_provider_ids: number[];
    video_provider_id: number | null;
}> {
    const response = await request.get(`${APP_URL}/api/ai/settings`);
    await ok(response);
    return await response.json();
}

export async function updateAiSettings(
    request: APIRequestContext,
    body: Record<string, unknown>,
): Promise<void> {
    const response = await request.put(`${APP_URL}/api/ai/settings`, {
        data: body,
    });
    await ok(response);
}

export async function enablePrivateUrls(
    request: APIRequestContext,
): Promise<unknown> {
    const current = await request.get(`${APP_URL}/api/admin/ingestion`);
    await ok(current);
    const settings = await current.json();
    const response = await request.put(`${APP_URL}/api/admin/ingestion`, {
        data: { ...settings, allow_private: true },
    });
    await ok(response);
    return settings;
}

export async function restoreIngestionSettings(
    request: APIRequestContext,
    settings: unknown,
): Promise<void> {
    const response = await request.put(`${APP_URL}/api/admin/ingestion`, {
        data: settings,
    });
    await ok(response);
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
    // Let the scheduler write the run record before the first poll, otherwise an immediate
    // status check could observe run===null (not-yet-started) and return early. waitForItems
    // is the real safety net, but this avoids the theoretical race.
    await new Promise((resolve) => setTimeout(resolve, 250));
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

/** Seed a plain-text item because rendered HTML intentionally has no summary action. */
export async function seedSummaryFeed(
    request: APIRequestContext,
): Promise<{ feedId: number; itemId: number }> {
    const feedId = await subscribeFeed(request, FIXTURE.summary, {
        kind: "jsonfeed",
    });
    await ingestNow(request);
    const items = await waitForItems(request, {
        predicate: (items) =>
            items.some(
                (item) =>
                    (item as { title?: string }).title ===
                    FIXTURE.summaryItemTitle,
            ),
    });
    const item = items.find(
        (candidate) =>
            (candidate as { title?: string }).title ===
            FIXTURE.summaryItemTitle,
    ) as { id?: number } | undefined;
    if (item?.id === undefined) {
        throw new Error("expected the summary fixture item");
    }
    return { feedId, itemId: item.id };
}

export async function seedYoutubeFeed(
    request: APIRequestContext,
): Promise<{ feedId: number; itemId: number }> {
    const feedId = await subscribeFeed(request, FIXTURE.youtube, {
        kind: "youtube",
    });
    await ingestNow(request);
    const items = await waitForItems(request, {
        predicate: (items) =>
            items.some(
                (item) =>
                    (item as { title?: string }).title === YOUTUBE_ITEM_TITLE,
            ),
    });
    const item = items.find(
        (candidate) =>
            (candidate as { title?: string }).title === YOUTUBE_ITEM_TITLE,
    ) as { id?: number } | undefined;
    if (item?.id === undefined) {
        throw new Error("expected the YouTube fixture item");
    }
    return { feedId, itemId: item.id };
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
