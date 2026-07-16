// items.spec.ts — SLICE 3 (specs-feeds)
// Seeds a feed+items via the API (so setup is deterministic) then drives the item UI:
// open preview, toggle mark-read, filter by topic chip, search.
import { expect, test } from "@playwright/test";
import {
    APP_URL,
    FIXTURE,
    registerUser,
    seedFeedWithItems,
} from "./support/api";

test.describe("items", () => {
    // Each test registers a unique user so ingest cooldowns and read-state don't bleed.
    test.beforeEach(async ({ page }) => {
        await registerUser(page.request);
        await seedFeedWithItems(page.request);
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
});
