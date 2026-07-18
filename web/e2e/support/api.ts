// Seed helpers + constants shared by every e2e spec. Specs seed their prerequisites through the
// real API (register, subscribe, ingest, admin digest run) and drive the UI only for the behavior
// they actually assert, per the approved spec (stage-2 "Test isolation & setup strategy").
//
// Cookie contract: `page.request` shares the browser context's cookie jar with `page`, so seeding
// the spec's own user via `page.request` leaves `page.goto()` already authenticated - no
// storageState needed. A second identity (e.g. admin) must use `request.newContext({ baseURL })`
// so its `hf_session` never clobbers the page user's.
import {
    type APIRequestContext,
    expect,
    type Page,
    request,
} from "@playwright/test";
import type {
    AdminSettings,
    Category,
    DigestListItem,
    FeedKind,
    PutNotifications,
} from "../../src/lib/types";
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
    /** Always returns 500 - use with seedFailingFeed to put a feed into "failing" health status. */
    failing500: "http://localhost:8098/fail/500",
    rssFeedTitle: RSS_FEED_TITLE,
    rssItemTitle: RSS_ITEM_TITLE,
    summaryItemTitle: SUMMARY_ITEM_TITLE,
    aiBaseUrl: "http://localhost:8098/ai-mock/openai",
    aiAnthropicBaseUrl: "http://localhost:8098/ai-mock/anthropic",
    /** The fake ntfy server: POST {ntfyBaseUrl}/{topic} is what notify::send hits (server/notify). */
    ntfyBaseUrl: "http://localhost:8098/ntfy",
};

export type AiMockResponse = {
    status: number;
    text?: string;
    error?: string;
    /** Delay before responding, e.g. to force a client-side timeout. */
    delayMs?: number;
    /** Respond 200 with a body the client can't parse, instead of the normal shape. */
    malformed?: boolean;
};

export type AiProviderInput = {
    name: string;
    provider_type: string;
    model: string;
    /** Defaults to "openai". Pass "anthropic" to exercise the fixture's /messages route. */
    api_style?: string;
};

let usernameCounter = 0;

/** Unique per call → order-independent tests and dodges the 60s per-user ingest cooldown.
 *  Includes `process.pid` so two Playwright worker *processes* (each with its own
 *  `usernameCounter` starting at 0) can't mint the same value for the same millisecond - a real
 *  collision once the suite runs with more than one worker (issue #43 shard/parallelize work). */
export function uniqueUsername(prefix = "e2e"): string {
    usernameCounter += 1;
    return `${prefix}${process.pid.toString(36)}${Date.now().toString(36)}${usernameCounter}`;
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
 * A separate, logged-in-as-admin request context. Formalizes the cookie-jar contract documented
 * at the top of this file: the caller owns the returned context and MUST `dispose()` it, so its
 * `hf_session` never clobbers a spec's own logged-in user. Prefer `withAdmin` for a one-off
 * action - reach for `asAdmin` directly only when the context needs to outlive a single call.
 */
export async function asAdmin(): Promise<APIRequestContext> {
    const admin = await request.newContext({ baseURL: APP_URL });
    try {
        await loginAs(admin, ADMIN.username, ADMIN.password);
    } catch (e) {
        await admin.dispose();
        throw e;
    }
    return admin;
}

/** A separate admin session keeps the browser user's session untouched during e2e setup. */
export async function withAdmin<T>(
    action: (admin: APIRequestContext) => Promise<T>,
): Promise<T> {
    const admin = await asAdmin();
    try {
        return await action(admin);
    } finally {
        await admin.dispose();
    }
}

/** Asserts a sonner toast bearing this text is visible. Toasts render inside sonner's own
 *  `[data-sonner-toast]` wrapper (components/ui/sonner.tsx, vendored - not edited for this),
 *  which scopes the assertion so it can't collide with the same text appearing elsewhere on the
 *  page. Centralizes toast-copy assertions so a copy tweak is a one-line fix here, not ~40. */
export async function expectToast(page: Page, text: string): Promise<void> {
    await expect(
        page.locator("[data-sonner-toast]").filter({ hasText: text }),
    ).toBeVisible();
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

/** Create an AI provider through the real admin endpoint, pointing it at the F1 fake. */
export async function seedAiProvider(
    request: APIRequestContext,
    input: AiProviderInput,
): Promise<number> {
    const apiStyle = input.api_style ?? "openai";
    const response = await request.post(`${APP_URL}/api/ai/providers`, {
        data: {
            ...input,
            api_style: apiStyle,
            base_url:
                apiStyle === "anthropic"
                    ? FIXTURE.aiAnthropicBaseUrl
                    : FIXTURE.aiBaseUrl,
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

export type NtfyReceipt = {
    topic: string;
    title: string | null;
    priority: string | null;
    tags: string | null;
    click: string | null;
    hasAuth: boolean;
    body: string;
};

/** GET the fake ntfy server's recorded pushes, so a spec can assert a notification actually fired. */
export async function ntfyReceipts(
    request: APIRequestContext,
): Promise<NtfyReceipt[]> {
    const response = await request.get("http://localhost:8098/ntfy/_received");
    await ok(response);
    return ((await response.json()) as { receipts: NtfyReceipt[] }).receipts;
}

/** Clears every fixture server's injected state (AI overrides, ntfy receipts, flaky counters). */
export async function resetFixtures(request: APIRequestContext): Promise<void> {
    const response = await request.post("http://localhost:8098/_control/reset");
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

/** POST /api/categories - a fresh category for the calling user, typed against Category so a
 *  serde rename on the DTO fails this at compile time rather than at runtime. */
export async function seedCategory(
    request: APIRequestContext,
    name: string,
): Promise<number> {
    const response = await request.post(`${APP_URL}/api/categories`, {
        data: { name },
    });
    await ok(response);
    const category: Category = await response.json();
    return category.id;
}

/** POST /api/feeds - direct subscribe (category is mandatory server-side). */
export async function subscribeFeed(
    request: APIRequestContext,
    feedUrl: string,
    opts?: { kind?: FeedKind; categoryId?: number },
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

export type SeedFeedSpec = {
    feedUrl: string;
    kind?: FeedKind;
    categoryId?: number;
};

/** Bulk-subscribe several feeds of (potentially) varied kinds in one call, returning their ids
 *  in the same order. Does not ingest - pair with `ingestNow` when items are needed. */
export async function seedFeeds(
    request: APIRequestContext,
    specs: SeedFeedSpec[],
): Promise<number[]> {
    const ids: number[] = [];
    for (const spec of specs) {
        ids.push(
            await subscribeFeed(request, spec.feedUrl, {
                kind: spec.kind,
                categoryId: spec.categoryId,
            }),
        );
    }
    return ids;
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

export type SeedItemState = {
    itemId: number;
    isRead?: boolean;
    isStarred?: boolean;
};

/**
 * POST /api/items/{id}/read and/or /star to set a known read/star state before a spec drives the
 * UI. Only read/star are covered: a Reddit item's `score` cannot be produced through a
 * fixture-reachable ingest path (backend/src/ingest/scheduler.rs::process_reddit always polls
 * hardcoded reddit.com URLs - see KNOWN BACKEND LIMITATIONS in the epic briefing), so there is no
 * seed path that lets a spec assert on a scored item's min_score filtering.
 */
export async function seedItems(
    request: APIRequestContext,
    states: SeedItemState[],
): Promise<void> {
    for (const state of states) {
        if (state.isRead !== undefined) {
            const response = await request.post(
                `${APP_URL}/api/items/${state.itemId}/read`,
                { data: { value: state.isRead } },
            );
            await ok(response);
        }
        if (state.isStarred !== undefined) {
            const response = await request.post(
                `${APP_URL}/api/items/${state.itemId}/star`,
                { data: { value: state.isStarred } },
            );
            await ok(response);
        }
    }
}

/** PUT /api/notifications, pointing ntfy at the F1 fake so a spec can assert on
 *  `ntfyReceipts()`. Returns the topic (generated unless one is passed) so the caller can
 *  scope its assertions to receipts from this seed call. */
export async function seedNtfy(
    request: APIRequestContext,
    opts?: {
        topic?: string;
        authToken?: string;
        priority?: number;
        notifyOnDigest?: boolean;
        notifyOnFeedHealth?: boolean;
    },
): Promise<{ topic: string }> {
    const topic = opts?.topic ?? uniqueUsername("e2e-ntfy");
    const body: PutNotifications = {
        ntfy_server_url: FIXTURE.ntfyBaseUrl,
        ntfy_topic: topic,
    };
    if (opts?.priority !== undefined) body.ntfy_priority = opts.priority;
    if (opts?.notifyOnDigest !== undefined)
        body.notify_on_digest = opts.notifyOnDigest;
    if (opts?.notifyOnFeedHealth !== undefined)
        body.notify_on_feed_health = opts.notifyOnFeedHealth;
    if (opts?.authToken !== undefined) body.auth_token = opts.authToken;

    const response = await request.put(`${APP_URL}/api/notifications`, {
        data: body,
    });
    await ok(response);
    return { topic };
}

/** GET then PUT /api/admin/settings, merging `patch` over the current settings (the same
 *  get-merge-put pattern `enablePrivateUrls` uses for the admin ingestion resource). Callers
 *  use a context from `asAdmin`/`withAdmin` since this is an admin-only endpoint. */
export async function seedAdminSetting(
    admin: APIRequestContext,
    patch: Partial<AdminSettings>,
): Promise<AdminSettings> {
    const current = await admin.get(`${APP_URL}/api/admin/settings`);
    await ok(current);
    const settings: AdminSettings = await current.json();
    const next: AdminSettings = { ...settings, ...patch };
    const response = await admin.put(`${APP_URL}/api/admin/settings`, {
        data: next,
    });
    await ok(response);
    return next;
}

/** Subscribe a feed that always 500s and ingest once - failure_count > 0 puts it straight into
 *  "failing" health status (backend/src/routes/feeds.rs: status is "failing" whenever
 *  failure_count > 0 and the feed isn't yet disabled), so a single ingest is enough. */
export async function seedFailingFeed(
    request: APIRequestContext,
    opts?: { categoryId?: number },
): Promise<{ feedId: number }> {
    const feedId = await subscribeFeed(request, FIXTURE.failing500, {
        categoryId: opts?.categoryId,
    });
    await ingestNow(request);
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

/** Seed a feed with items, run the admin digest, and return the caller's newest digest id - the
 *  full precondition most digest specs need in one call. Typed against DigestListItem so a serde
 *  rename on that DTO fails this at compile time. */
export async function seedDigest(
    request: APIRequestContext,
    opts?: { lookbackDays?: number },
): Promise<{ feedId: number; digestId: number }> {
    const { feedId } = await seedFeedWithItems(request);
    await adminDigestRun({ lookbackDays: opts?.lookbackDays });

    const response = await request.get(`${APP_URL}/api/digest`);
    await ok(response);
    const digests: DigestListItem[] = await response.json();
    if (digests.length === 0) {
        throw new Error("expected at least one digest after adminDigestRun");
    }
    return { feedId, digestId: digests[0].id };
}
