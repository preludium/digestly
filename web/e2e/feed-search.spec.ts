// feed-search.spec.ts — issue #45 (E2E: feed reading)
// The Feed screen's search box: debounced ?q= writes, Clear, the two distinct empty-state
// copies, title/snippet highlighting, and suppression while the preview sheet is open
// (Feed.tsx:89-97 - the debounce effect is a no-op while `previewId != null`).
import { expect, test } from "@playwright/test";
import { FIXTURE, registerUser, seedFeedWithItems } from "./support/api";

test.describe("feed search", () => {
    test.beforeEach(async ({ page }) => {
        await registerUser(page.request);
        await seedFeedWithItems(page.request);
        await page.goto("/");
    });

    test("typing debounces to a single settled request and writes ?q=", async ({
        page,
    }) => {
        const cardOne = page
            .getByTestId("item-card")
            .filter({ hasText: FIXTURE.rssItemTitle });
        const cardTwo = page
            .getByTestId("item-card")
            .filter({ hasText: "E2E Fixture RSS Item Two" });
        await expect(cardOne).toBeVisible();
        await expect(cardTwo).toBeVisible();

        const itemRequests: string[] = [];
        page.on("request", (req) => {
            const url = new URL(req.url());
            if (url.pathname === "/api/items" && url.searchParams.has("q")) {
                itemRequests.push(url.search);
            }
        });

        // Type character-by-character, faster than the 300ms debounce (Feed.tsx:95), so the
        // debounce timer keeps resetting instead of firing once per keystroke.
        await page
            .getByLabel("Search articles")
            .pressSequentially("Item One", { delay: 40 });

        // Only "Item One" should remain once the debounced query lands.
        await expect(cardOne).toBeVisible();
        await expect(cardTwo).toBeHidden();
        await expect(page).toHaveURL(/[?&]q=Item(\+|%20)One/);

        // Asserting the debounce itself: confirm exactly one request settled, not one per
        // keystroke ("Item One" is 8 keystrokes).
        expect(itemRequests).toHaveLength(1);
        expect(itemRequests[0]).toContain("q=Item");
    });

    test("Clear restores the full list", async ({ page }) => {
        const cardOne = page
            .getByTestId("item-card")
            .filter({ hasText: FIXTURE.rssItemTitle });
        const cardTwo = page
            .getByTestId("item-card")
            .filter({ hasText: "E2E Fixture RSS Item Two" });

        await page.getByLabel("Search articles").fill("RSS Item One");
        await expect(cardOne).toBeVisible();
        await expect(cardTwo).toBeHidden();

        await page.getByLabel("Search articles").clear();
        await expect(cardOne).toBeVisible();
        await expect(cardTwo).toBeVisible();
        await expect(page).not.toHaveURL(/[?&]q=/);
    });

    test("no matches shows the search-specific empty state", async ({
        page,
    }) => {
        await page.getByLabel("Search articles").fill("zzz-no-such-item-zzz");
        await expect(
            page.getByText('No results for "zzz-no-such-item-zzz"'),
        ).toBeVisible();
        // Distinct from the no-filter-match copy (feed-filters.spec.ts covers that one).
        await expect(
            page.getByText("Nothing matches these filters"),
        ).toBeHidden();
    });

    test("highlights the matched terms in card titles and snippets", async ({
        page,
    }) => {
        await page.getByLabel("Search articles").fill("Fixture Item");
        const card = page
            .getByTestId("item-card")
            .filter({ hasText: FIXTURE.rssItemTitle });
        await expect(card).toBeVisible();
        // highlight() (lib/highlight.tsx) wraps each matched term in its own <mark>, case-
        // insensitively - both the title ("Fixture"/"Item") and the snippet ("fixture"/"item" in
        // "First fixture item body.") match, so scope by element to avoid a strict-mode
        // collision between the two "Fixture" marks.
        await expect(card.locator("h3 mark")).toHaveCount(2);
        expect(await card.locator("h3 mark").allTextContents()).toEqual([
            "Fixture",
            "Item",
        ]);
        await expect(card.locator("p mark")).toHaveCount(2);
        expect(await card.locator("p mark").allTextContents()).toEqual([
            "fixture",
            "item",
        ]);
    });

    test("search is not re-triggered while the preview sheet is open", async ({
        page,
    }) => {
        const card = page
            .getByTestId("item-card")
            .filter({ hasText: FIXTURE.rssItemTitle });
        await card.click();
        await expect(page).toHaveURL(/[?&]item=\d+/);

        const itemRequests: string[] = [];
        page.on("request", (req) => {
            const url = new URL(req.url());
            if (url.pathname === "/api/items" && url.searchParams.has("q")) {
                itemRequests.push(url.search);
            }
        });

        // The search input can't be typed into normally here: the preview Sheet is modal (Radix
        // Dialog default), so it both covers the input with its full-viewport overlay AND traps
        // focus inside itself, reclaiming focus if we try to move it out via `.focus()`. The
        // debounce-suppression behavior under test is gated purely on `previewId` (Feed.tsx:94),
        // not on how the value changes, so drive the same React-controlled input the way React
        // Testing Library's `fireEvent` does: set the value through the native setter (bypassing
        // React's own tracked-value shadowing) and dispatch a real "input" event.
        await page
            .getByLabel("Search articles")
            .evaluate((el: HTMLInputElement) => {
                const setter = Object.getOwnPropertyDescriptor(
                    window.HTMLInputElement.prototype,
                    "value",
                )?.set;
                setter?.call(el, "Item One");
                el.dispatchEvent(new Event("input", { bubbles: true }));
            });

        // Wait past the debounce window; nothing should fire while the preview is open.
        await page.waitForTimeout(400);
        expect(itemRequests).toHaveLength(0);

        // Closing the preview flushes the held text - the debounce effect re-runs because
        // `previewId` is a dependency (Feed.tsx:97).
        await page.keyboard.press("Escape");
        await expect(page).not.toHaveURL(/[?&]item=/);
        await expect(page).toHaveURL(/[?&]q=Item(\+|%20)One/);
        expect(itemRequests).toHaveLength(1);
    });
});
