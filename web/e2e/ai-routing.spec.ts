import { expect, test } from "@playwright/test";
import {
    ADMIN,
    aiMockRequests,
    aiSettings,
    configureAiMock,
    deleteAiProvider,
    enablePrivateUrls,
    loginAs,
    registerUser,
    resetAiMock,
    restoreIngestionSettings,
    seedAiProvider,
    seedSummaryFeed,
    seedYoutubeFeed,
    updateAiSettings,
    withAdmin,
} from "./support/api";

let originalIngestionSettings: unknown;
let createdProviderIds: number[] = [];

test.describe("AI provider routing", () => {
    test.beforeAll(async () => {
        originalIngestionSettings = await withAdmin(enablePrivateUrls);
    });

    test.afterAll(async () => {
        await withAdmin((admin) =>
            restoreIngestionSettings(admin, originalIngestionSettings),
        );
    });

    test.afterEach(async ({ page }) => {
        await resetAiMock(page.request);
        await withAdmin(async (admin) => {
            await updateAiSettings(admin, {
                text_provider_mode: "single",
                text_provider_ids: [],
                video_provider_id: null,
            });
            for (const providerId of createdProviderIds) {
                await deleteAiProvider(admin, providerId);
            }
        });
        createdProviderIds = [];
    });

    async function createProvider(input: Parameters<typeof seedAiProvider>[1]) {
        const id = await withAdmin((admin) => seedAiProvider(admin, input));
        createdProviderIds.push(id);
        return id;
    }

    test("single mode selects a provider and persists", async ({ page }) => {
        const primary = await createProvider({
            name: "Single primary",
            provider_type: "custom",
            model: "single-primary",
        });
        const secondary = await createProvider({
            name: "Single secondary",
            provider_type: "custom",
            model: "single-secondary",
        });

        await withAdmin((admin) =>
            updateAiSettings(admin, {
                text_provider_mode: "single",
                text_provider_ids: [primary],
            }),
        );
        await loginAs(page.request, ADMIN.username, ADMIN.password);
        await page.goto("/admin/system");
        await page.getByRole("button", { name: "AI" }).click();
        await expect(
            page.getByRole("heading", { name: "Text providers" }),
        ).toBeVisible();
        await expect(page.getByLabel("Text provider mode")).toHaveText(
            "Single provider",
        );

        await page.locator("#text-provider").click();
        await page
            .getByRole("option", {
                name: "Single secondary (single-secondary)",
            })
            .click();
        await expect
            .poll(async () => (await withAdmin(aiSettings)).text_provider_ids)
            .toEqual([secondary]);

        await page.reload();
        await page.getByRole("button", { name: "AI" }).click();
        await expect(page.locator("#text-provider")).toContainText(
            "Single secondary",
        );
    });

    test("ordered mode reorders providers and persists", async ({ page }) => {
        const first = await createProvider({
            name: "Ordered first",
            provider_type: "custom",
            model: "ordered-first",
        });
        const second = await createProvider({
            name: "Ordered second",
            provider_type: "custom",
            model: "ordered-second",
        });
        await withAdmin((admin) =>
            updateAiSettings(admin, {
                text_provider_mode: "single",
                text_provider_ids: [first],
            }),
        );
        await loginAs(page.request, ADMIN.username, ADMIN.password);
        await page.goto("/admin/system");
        await page.getByRole("button", { name: "AI" }).click();
        await page.getByLabel("Text provider mode").click();
        await page.getByRole("option", { name: "Ordered fallback" }).click();
        await page.locator("#fallback-provider").click();
        await page
            .getByRole("option", { name: "Ordered second (ordered-second)" })
            .click();
        await expect
            .poll(async () => (await withAdmin(aiSettings)).text_provider_ids)
            .toEqual([first, second]);

        await page
            .getByRole("button", { name: "Move Ordered second up" })
            .click();
        await expect
            .poll(async () => (await withAdmin(aiSettings)).text_provider_ids)
            .toEqual([second, first]);
        await page.reload();
        await page.getByRole("button", { name: "AI" }).click();
        await expect(
            page.getByText("Ordered second (ordered-second)").first(),
        ).toBeVisible();
    });

    test("falls back after a non-retryable provider error", async ({
        page,
    }) => {
        const failing = await createProvider({
            name: "Fails 400",
            provider_type: "custom",
            model: "fails-400",
        });
        const succeeding = await createProvider({
            name: "Succeeds",
            provider_type: "custom",
            model: "succeeds",
        });
        await withAdmin(async (admin) => {
            await updateAiSettings(admin, {
                text_provider_mode: "ordered",
                text_provider_ids: [failing, succeeding],
            });
            await configureAiMock(admin, {
                "fails-400": { status: 400, error: "invalid request" },
                succeeds: { status: 200, text: "Fallback summary succeeded." },
            });
        });
        await registerUser(page.request);
        const { itemId } = await seedSummaryFeed(page.request);
        const summary = await page.request.post(
            `/api/items/${itemId}/summarize`,
        );
        expect(summary.ok()).toBe(true);
        await expect(summary.json()).resolves.toMatchObject({
            summary: "Fallback summary succeeded.",
            model: "succeeds",
            cached: false,
        });
        expect(await withAdmin(aiMockRequests)).toEqual([
            { kind: "openai", model: "fails-400" },
            { kind: "openai", model: "succeeds" },
        ]);
    });

    test("reports an error when every text provider fails", async ({
        page,
    }) => {
        const first = await createProvider({
            name: "All fail first",
            provider_type: "custom",
            model: "all-fail-first",
        });
        const second = await createProvider({
            name: "All fail second",
            provider_type: "custom",
            model: "all-fail-second",
        });
        await withAdmin(async (admin) => {
            await updateAiSettings(admin, {
                text_provider_mode: "ordered",
                text_provider_ids: [first, second],
            });
            await configureAiMock(admin, {
                "all-fail-first": { status: 400, error: "first failure" },
                "all-fail-second": { status: 400, error: "second failure" },
            });
        });
        await registerUser(page.request);
        const { itemId } = await seedSummaryFeed(page.request);
        const summary = await page.request.post(
            `/api/items/${itemId}/summarize`,
        );
        expect(summary.status()).toBe(502);
        await expect(summary.json()).resolves.toMatchObject({
            error: "AI provider returned an error (400): second failure",
        });
        expect(await withAdmin(aiMockRequests)).toEqual([
            { kind: "openai", model: "all-fail-first" },
            { kind: "openai", model: "all-fail-second" },
        ]);
    });

    test("dedicated Gemini video selection excludes it from text routing", async ({
        page,
    }) => {
        const text = await createProvider({
            name: "Text route",
            provider_type: "custom",
            model: "text-route",
        });
        const gemini = await createProvider({
            name: "Gemini video",
            provider_type: "gemini",
            model: "gemini-video",
        });
        await withAdmin((admin) =>
            updateAiSettings(admin, {
                text_provider_mode: "ordered",
                text_provider_ids: [text],
                video_provider_id: gemini,
            }),
        );
        await loginAs(page.request, ADMIN.username, ADMIN.password);
        await page.goto("/admin/system");
        await page.getByRole("button", { name: "AI" }).click();
        await expect(
            page.getByText("Gemini video (gemini-video)"),
        ).toBeVisible();
        await expect(page.locator("#fallback-provider")).not.toContainText(
            "Gemini video",
        );
        expect(await withAdmin(aiSettings)).toMatchObject({
            text_provider_mode: "ordered",
            text_provider_ids: [text],
            video_provider_id: gemini,
        });
    });

    test("dedicated Gemini summarizes a YouTube item through the native route", async ({
        page,
    }) => {
        const text = await createProvider({
            name: "Text route",
            provider_type: "custom",
            model: "text-route-success",
        });
        const gemini = await createProvider({
            name: "Gemini video",
            provider_type: "gemini",
            model: "gemini-video-success",
        });
        await withAdmin(async (admin) => {
            await updateAiSettings(admin, {
                text_provider_mode: "single",
                text_provider_ids: [text],
                video_provider_id: gemini,
            });
            await configureAiMock(admin, {
                "gemini-video-success": {
                    status: 200,
                    text: "Native Gemini video summary.",
                },
            });
        });
        await registerUser(page.request);
        const { itemId } = await seedYoutubeFeed(page.request);

        const summary = await page.request.post(
            `/api/items/${itemId}/summarize`,
        );
        expect(summary.ok()).toBe(true);
        await expect(summary.json()).resolves.toMatchObject({
            summary: "Native Gemini video summary.",
            model: "gemini-video-success",
            cached: false,
        });
        expect(await withAdmin(aiMockRequests)).toEqual([
            { kind: "gemini", model: "gemini-video-success" },
        ]);
    });
});
