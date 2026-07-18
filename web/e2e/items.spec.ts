// items.spec.ts — SLICE 3 (specs-feeds)
// Seeds a feed+items via the API (so setup is deterministic) then drives the item UI:
// open preview, toggle mark-read, filter by topic chip, search.
import { expect, test } from "@playwright/test";
import {
    APP_URL,
    FIXTURE,
    registerUser,
    seedFeedWithItems,
    seedYoutubeFeed,
} from "./support/api";

test.describe("items", () => {
    // Each test registers a unique user so ingest cooldowns and read-state don't bleed.
    test.beforeEach(async ({ page }, testInfo) => {
        await registerUser(page.request);
        if (testInfo.titlePath.includes("video topics")) {
            await seedYoutubeFeed(page.request);
        } else {
            await seedFeedWithItems(page.request);
        }
        await page.goto("/");
    });

    test("open preview and toggle mark read / unread", async ({ page }) => {
        // ItemCard is a <button> with the item title inside an <h3>. Filter by the h3 text
        // to avoid matching on the concatenated accessible name (title + feed + snippet + badges).
        const card = page.getByRole("button").filter({
            has: page.locator("h3", { hasText: FIXTURE.rssItemTitle }),
        });
        await expect(card).toBeVisible();
        await card.click();

        // ItemPreview Sheet opens; URL gets ?item=<id>. auto_mark_read defaults to false
        // (backend/src/routes/settings.rs:92). Opening the Sheet itself does NOT call
        // markReadOnOpen — that only fires when "Open original" is clicked (ItemPreview.tsx:233).
        // So the item is still unread immediately after the sheet opens.
        await expect(
            page.getByRole("button", { name: "Mark as read" }),
        ).toBeVisible();

        // Toggle to read.
        await page.getByRole("button", { name: "Mark as read" }).click();
        await expect(
            page.getByRole("button", { name: "Mark as unread" }),
        ).toBeVisible();

        // Toggle back to unread.
        await page.getByRole("button", { name: "Mark as unread" }).click();
        await expect(
            page.getByRole("button", { name: "Mark as read" }),
        ).toBeVisible();
    });

    test("filter by topic chip selects and resets", async ({ page }) => {
        // seedFeedWithItems subscribes the fixture feed to the first category; fetch its name.
        const catsRes = await page.request.get(`${APP_URL}/api/categories`);
        const cats = (await catsRes.json()) as Array<{
            id: number;
            name: string;
        }>;
        const catName = cats[0].name; // "Other" per seed.rs DEFAULT_CATEGORIES[0]

        const card = page.getByRole("button").filter({
            has: page.locator("h3", { hasText: FIXTURE.rssItemTitle }),
        });
        await expect(card).toBeVisible();

        // Topic chips are <button>s whose accessible name starts with the category name
        // followed by an item count, e.g. "Other 2". ItemCard buttons start with the item
        // title, so /^{catName}/ selects the chip without colliding with item cards.
        await page
            .getByRole("button", { name: new RegExp(`^${catName}`) })
            .click();
        await expect(card).toBeVisible();

        // "All topics" resets the category filter (FilterBar.tsx:155-164).
        await page.getByRole("button", { name: /^All topics/ }).click();
        await expect(card).toBeVisible();
    });

    test("search filters to the item and clears", async ({ page }) => {
        const card = page.getByRole("button").filter({
            has: page.locator("h3", { hasText: FIXTURE.rssItemTitle }),
        });
        await expect(card).toBeVisible();

        // Search is debounced 300 ms (Feed.tsx:94) then appended as ?q= to /api/items.
        // "RSS Item One" uniquely matches "E2E Fixture RSS Item One", not "Item Two".
        await page.getByLabel("Search articles").fill("RSS Item One");
        await expect(card).toBeVisible();

        // Clear the search; all items return.
        await page.getByLabel("Search articles").clear();
        await expect(card).toBeVisible();
    });

    test.describe("video topics", () => {
        test.use({ serviceWorkers: "block" });

        test("label the source and keep captions collapsed", async ({
            page,
        }) => {
            const items = await page.request.get(
                `${APP_URL}/api/items?type=video`,
            );
            const { id: itemId } = (
                (await items.json()) as {
                    items: Array<{ id: number }>;
                }
            ).items[0];
            const detail = await page.request.get(
                `${APP_URL}/api/items/${itemId}`,
            );
            const video = (await detail.json()) as Record<string, unknown>;
            const videoTitle = String(video.title);
            const topicSummary =
                "- **Performance:** The video explains how to measure bottlenecks.";

            await page.route(`**/api/items/${itemId}`, async (route) => {
                await route.fulfill({
                    json: {
                        ...video,
                        summary: topicSummary,
                        summary_kind: "text-video-topics-v1",
                        transcript_status: "fetched",
                        transcript_text: "Caption reference text.",
                    },
                });
            });

            await page
                .getByRole("button")
                .filter({ has: page.locator("h3", { hasText: videoTitle }) })
                .click();
            await expect(page.getByText("Video topics")).toBeVisible();
            await expect(page.getByText("From captions")).toBeVisible();
            await expect(page.getByText("Performance:")).toBeVisible();
            const original = page.getByRole("link", {
                name: "Open original video",
            });
            await expect(original).toBeVisible();
            expect(
                await original.evaluate(
                    (link, title) =>
                        link.parentElement?.querySelector("h1")?.textContent ===
                        title,
                    videoTitle,
                ),
            ).toBe(true);
            await original.hover();
            await expect(page.getByText("Open original video")).toBeVisible();

            const transcript = page.getByRole("button", { name: "Transcript" });
            await expect(transcript).toHaveAttribute("aria-expanded", "false");
            await expect(
                page.getByText("Caption reference text."),
            ).toBeHidden();
            await transcript.click();
            await expect(transcript).toHaveAttribute("aria-expanded", "true");
            await expect(
                page.getByText("Caption reference text."),
            ).toBeVisible();

            await page.setViewportSize({ width: 375, height: 812 });
            await expect(transcript).toBeVisible();
            expect(
                await page
                    .locator("html")
                    .evaluate((html) => html.scrollWidth <= html.clientWidth),
            ).toBe(true);
        });

        test("labels description-only summaries", async ({ page }) => {
            const items = await page.request.get(
                `${APP_URL}/api/items?type=video`,
            );
            const { id: itemId } = (
                (await items.json()) as {
                    items: Array<{ id: number }>;
                }
            ).items[0];
            const detail = await page.request.get(
                `${APP_URL}/api/items/${itemId}`,
            );
            const video = (await detail.json()) as Record<string, unknown>;
            const videoTitle = String(video.title);

            await page.route(`**/api/items/${itemId}`, async (route) => {
                await route.fulfill({
                    json: {
                        ...video,
                        summary:
                            "- **Setup:** The description introduces the subject.",
                        summary_kind: "text-video-topics-v1",
                        transcript_status: "unavailable",
                        transcript_text: null,
                    },
                });
            });

            await page
                .getByRole("button")
                .filter({ has: page.locator("h3", { hasText: videoTitle }) })
                .click();
            await expect(
                page.getByText("Based only on the video description"),
            ).toBeVisible();
        });
    });
});
