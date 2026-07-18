// items.spec.ts — SLICE 3 (specs-feeds)
// Seeds a feed+items via the API (so setup is deterministic) then drives the item UI:
// open preview, toggle mark-read, filter by topic chip, search.
import { expect, test } from "@playwright/test";
import {
    FIXTURE,
    firstCategoryId,
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
        // ItemCard sets data-testid="item-card"; filter by title text to avoid matching on the
        // concatenated accessible name (title + feed + snippet + badges).
        const card = page
            .getByTestId("item-card")
            .filter({ hasText: FIXTURE.rssItemTitle });
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
        // seedFeedWithItems subscribes the fixture feed to the first category.
        const catId = await firstCategoryId(page.request);

        const card = page
            .getByTestId("item-card")
            .filter({ hasText: FIXTURE.rssItemTitle });
        await expect(card).toBeVisible();

        // Topic chips carry data-category-id ("all" or the category's id), so this selects the
        // chip by id rather than parsing the category name out of its "<name> <count>" label.
        await page
            .locator(`[data-testid="topic-chip"][data-category-id="${catId}"]`)
            .click();
        await expect(card).toBeVisible();

        // "All topics" resets the category filter (FilterBar.tsx:155-164).
        await page
            .locator('[data-testid="topic-chip"][data-category-id="all"]')
            .click();
        await expect(card).toBeVisible();
    });

    test("search filters to the item and clears", async ({ page }) => {
        const card = page
            .getByTestId("item-card")
            .filter({ hasText: FIXTURE.rssItemTitle });
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
