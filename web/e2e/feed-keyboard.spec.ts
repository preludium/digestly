// feed-keyboard.spec.ts — issue #45 (E2E: feed reading)
// Feed.tsx's window keydown handler (§9.1): n/p page (no-op with the preview open), r ingests
// even with the preview open, / focuses search (no-op with the preview open), o/m/s act on the
// open preview item, and every shortcut is ignored while focus is in a form control or a
// meta/ctrl/alt modifier is held.
import { expect, test } from "@playwright/test";
import {
    FIXTURE,
    ingestNow,
    itemIdByTitle,
    registerUser,
    seedFeeds,
    setPageSize,
    waitForItems,
} from "./support/api";

test.describe("feed keyboard shortcuts", () => {
    // Default page_size (50): every test but "n/p" needs to locate a SPECIFIC item by title on
    // screen, which a small page could easily exclude. "n/p" opts into page_size=1 itself, since
    // it's the one test that needs more than one page.
    test.beforeEach(async ({ page }) => {
        await registerUser(page.request);
        await seedFeeds(page.request, [
            { feedUrl: FIXTURE.rss },
            { feedUrl: FIXTURE.atom },
        ]);
        await ingestNow(page.request);
        await waitForItems(page.request, {
            predicate: (items) =>
                items.some(
                    (i) =>
                        (i as { title?: string }).title ===
                        "E2E Fixture Atom Item One",
                ),
        });
        await page.goto("/");
        // n/p read `items.data.total_pages` (Feed.tsx), which is undefined until the first
        // /api/items response lands - wait for the grid so every test's first keypress is
        // guaranteed to see it populated.
        await expect(page.getByTestId("item-card").first()).toBeVisible();
    });

    test("n/p page forward and back, and are no-ops while the preview is open", async ({
        page,
    }) => {
        // rss + atom = 4 items = 4 pages at page_size=1, enough room for n/p.
        await setPageSize(page.request, 1);
        await page.reload();
        await expect(page.getByTestId("item-card")).toHaveCount(1);

        await page.keyboard.press("n");
        await expect(page).toHaveURL(/[?&]page=2/);

        await page.getByTestId("item-card").first().click();
        await expect(page).toHaveURL(/[?&]item=\d+/);
        // Wait for the sheet itself, not just the URL: the keydown handler re-subscribes with the
        // new `previewId` in a passive effect, which can commit a tick after the URL updates -
        // pressing "n" before that lands would still hit the pre-open (stale) handler.
        await expect(page.getByRole("dialog")).toBeVisible();

        await page.keyboard.press("n");
        await expect(page).toHaveURL(/[?&]page=2/);
        await expect(page).not.toHaveURL(/[?&]page=3/);

        await page.keyboard.press("p");
        await expect(page).toHaveURL(/[?&]page=2/);
        await expect(page).toHaveURL(/[?&]item=\d+/);

        await page.keyboard.press("Escape");
        await expect(page.getByRole("dialog")).toBeHidden();
        await page.keyboard.press("p");
        await expect(page).not.toHaveURL(/[?&]page=/);
    });

    test("r ingests even with the preview open", async ({ page }) => {
        const refreshRequests: string[] = [];
        page.on("request", (req) => {
            if (req.url().endsWith("/api/feeds/refresh-all")) {
                refreshRequests.push(req.url());
            }
        });

        await page.getByTestId("item-card").first().click();
        await expect(page).toHaveURL(/[?&]item=\d+/);

        const refreshFired = page.waitForRequest("**/api/feeds/refresh-all");
        await page.keyboard.press("r");
        await refreshFired;
        expect(refreshRequests).toHaveLength(1);
    });

    test("/ focuses search, and is a no-op while the preview is open", async ({
        page,
    }) => {
        await page.keyboard.press("/");
        await expect(page.getByLabel("Search articles")).toBeFocused();

        // Clicking the card both opens the preview and (as a normal side effect of clicking a
        // focusable element) moves focus off the search input.
        await page.getByTestId("item-card").first().click();
        await expect(page).toHaveURL(/[?&]item=\d+/);

        await page.keyboard.press("/");
        await expect(page.getByLabel("Search articles")).not.toBeFocused();
    });

    test("o/m/s act on the open preview item", async ({ page, context }) => {
        const itemId = await itemIdByTitle(page.request, FIXTURE.rssItemTitle);
        const detail = await page.request.get(`/api/items/${itemId}`);
        const { url } = (await detail.json()) as { url: string };
        await context.route(url, (route) =>
            route.fulfill({
                status: 200,
                contentType: "text/html",
                body: "stub",
            }),
        );

        await page
            .getByTestId("item-card")
            .filter({ hasText: FIXTURE.rssItemTitle })
            .click();
        await expect(page).toHaveURL(new RegExp(`[?&]item=${itemId}`));
        await expect(
            page.getByRole("button", { name: "Mark as read" }),
        ).toBeVisible();

        await page.keyboard.press("s");
        await expect(
            page.getByRole("button", { name: "Unstar" }),
        ).toBeVisible();

        await page.keyboard.press("m");
        await expect(
            page.getByRole("button", { name: "Mark as unread" }),
        ).toBeVisible();

        const [popup] = await Promise.all([
            context.waitForEvent("page"),
            page.keyboard.press("o"),
        ]);
        await popup.close();
    });

    test("shortcuts are ignored while focus is in an input", async ({
        page,
    }) => {
        const refreshRequests: string[] = [];
        page.on("request", (req) => {
            if (req.url().endsWith("/api/feeds/refresh-all")) {
                refreshRequests.push(req.url());
            }
        });

        const search = page.getByLabel("Search articles");
        await search.click();
        // With focus already in the input, each key types literally instead of firing its
        // shortcut - "r" appends to the query, it does not trigger an ingest.
        await page.keyboard.type("rnp/", { delay: 20 });
        await expect(search).toHaveValue("rnp/");
        await expect(page).not.toHaveURL(/[?&]page=/);

        // Give a would-be ingest request a moment to show up before asserting its absence.
        await page.waitForTimeout(200);
        expect(refreshRequests).toHaveLength(0);
    });

    test("shortcuts are ignored with a meta/ctrl/alt modifier held", async ({
        page,
    }) => {
        const refreshRequests: string[] = [];
        page.on("request", (req) => {
            if (req.url().endsWith("/api/feeds/refresh-all")) {
                refreshRequests.push(req.url());
            }
        });

        await page.keyboard.press("Alt+n");
        await expect(page).not.toHaveURL(/[?&]page=/);

        await page.getByTestId("item-card").first().click();
        await expect(page).toHaveURL(/[?&]item=\d+/);
        await page.keyboard.press("Meta+m");
        await expect(
            page.getByRole("button", { name: "Mark as read" }),
        ).toBeVisible();

        await page.keyboard.press("Control+r");
        await page.waitForTimeout(200);
        expect(refreshRequests).toHaveLength(0);
    });
});
