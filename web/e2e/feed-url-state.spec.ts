// feed-url-state.spec.ts — issue #45 (E2E: feed reading)
// The URL is the single source of truth for the feed's filters (useFeedFilters.ts) and is meant
// to be shareable. These specs assert the query string DIRECTLY: defaults are omitted, a facet
// change resets ?page=, a page change preserves facets, a foreign param (?item=) survives a
// filter write, and reloading a filtered URL reconstructs the same view.
import { expect, test } from "@playwright/test";
import {
    FIXTURE,
    firstCategoryId,
    ingestNow,
    registerUser,
    seedFeeds,
    setPageSize,
    waitForItems,
} from "./support/api";

test.describe("feed URL state", () => {
    test("defaults are omitted from the URL", async ({ page }) => {
        await registerUser(page.request);
        await seedFeeds(page.request, [{ feedUrl: FIXTURE.rss }]);
        await ingestNow(page.request);
        await waitForItems(page.request, {
            predicate: (items) => items.length >= 2,
        });

        await page.goto("/");
        expect(new URL(page.url()).search).toBe("");

        // Explicitly setting a facet back to its default value must also leave a clean URL, not
        // a param that happens to equal the default.
        await page
            .locator('[data-testid="sort-pill"] [role="combobox"]')
            .click();
        await page.getByRole("option", { name: "Oldest" }).click();
        await expect(page).toHaveURL(/[?&]sort=old/);

        await page
            .locator('[data-testid="sort-pill"] [role="combobox"]')
            .click();
        await page.getByRole("option", { name: "Newest" }).click();
        expect(new URL(page.url()).search).toBe("");
    });

    test("a facet change resets page=1, and a page change preserves facets", async ({
        page,
    }) => {
        await registerUser(page.request);
        const catId = await firstCategoryId(page.request);
        await seedFeeds(page.request, [{ feedUrl: FIXTURE.rss }]);
        await ingestNow(page.request);
        await waitForItems(page.request, {
            predicate: (items) => items.length >= 2,
        });
        // Force a small page AFTER confirming both items exist: page_size clamps every
        // /api/items response (including waitForItems' own polling GETs above).
        await setPageSize(page.request, 1);

        await page.goto("/");
        await page
            .locator(`[data-testid="topic-chip"][data-category-id="${catId}"]`)
            .click();
        expect(new URL(page.url()).search).toBe(`?cat=${catId}`);

        await page.getByRole("button", { name: "Next page" }).click();
        // Page change preserves the active facet.
        expect(new URL(page.url()).search).toBe(`?cat=${catId}&page=2`);

        // A facet change from page 2 resets back to page 1 (dropped from the URL).
        await page
            .locator(
                '[data-testid="refine-pill"][data-variant="desktop"] [role="combobox"]',
            )
            .nth(1)
            .click();
        await page.getByRole("option", { name: "Unread" }).click();
        expect(new URL(page.url()).search).toBe(`?status=unread&cat=${catId}`);
    });

    test("?item= survives a filter write", async ({ page }) => {
        await registerUser(page.request);
        const catId = await firstCategoryId(page.request);
        await seedFeeds(page.request, [{ feedUrl: FIXTURE.rss }]);
        await ingestNow(page.request);
        await waitForItems(page.request, {
            predicate: (items) => items.length >= 2,
        });

        await page.goto("/");
        await page
            .getByTestId("item-card")
            .filter({ hasText: FIXTURE.rssItemTitle })
            .click();
        const itemMatch = page.url().match(/[?&]item=(\d+)/);
        expect(itemMatch).not.toBeNull();
        const itemId = itemMatch?.[1];

        // The preview Sheet is modal (Radix Dialog default): its full-viewport overlay physically
        // covers the topic chip, so a real pointer click can't reach it - Playwright would just
        // keep retrying against "element intercepts pointer events" until the test times out.
        // Dispatching the click event directly on the chip still exercises the same onClick/
        // setFacet/write() path a real click would, without needing to click through the overlay.
        await page
            .locator(`[data-testid="topic-chip"][data-category-id="${catId}"]`)
            .dispatchEvent("click");
        await expect(page).toHaveURL(new RegExp(`[?&]cat=${catId}`));
        expect(new URL(page.url()).search).toBe(`?cat=${catId}&item=${itemId}`);
    });

    test("reloading a filtered URL reconstructs the same view", async ({
        page,
    }) => {
        await registerUser(page.request);
        await seedFeeds(page.request, [{ feedUrl: FIXTURE.rss }]);
        await ingestNow(page.request);
        await waitForItems(page.request, {
            predicate: (items) => items.length >= 2,
        });

        await page.goto("/");
        await page
            .locator(
                '[data-testid="refine-pill"][data-variant="desktop"] [role="combobox"]',
            )
            .nth(2)
            .click();
        await page.getByRole("option", { name: "This week" }).click();
        await page
            .locator('[data-testid="sort-pill"] [role="combobox"]')
            .click();
        await page.getByRole("option", { name: "Oldest" }).click();

        const filteredUrl = page.url();
        expect(new URL(filteredUrl).search).toBe("?when=week&sort=old");
        await expect(page.getByTestId("item-card")).toHaveCount(2);

        await page.reload();

        expect(page.url()).toBe(filteredUrl);
        await expect(page.getByTestId("item-card")).toHaveCount(2);
        // The Select controls reconstruct the same visible state, not just the URL.
        await expect(
            page
                .locator('[data-testid="refine-pill"][data-variant="desktop"]')
                .getByText("This week"),
        ).toBeVisible();
        await expect(
            page.locator('[data-testid="sort-pill"]').getByText("Oldest"),
        ).toBeVisible();
    });
});
