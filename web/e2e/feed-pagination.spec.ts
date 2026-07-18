// feed-pagination.spec.ts — issue #45 (E2E: feed reading)
// Pagination.tsx (shared by the feed grid): hidden for a single page, Next/Previous updating
// ?page= with aria-current tracking the active page, and ellipsis windowing for many pages.
import { expect, test } from "@playwright/test";
import {
    FIXTURE,
    ingestNow,
    registerUser,
    seedFeeds,
    setPageSize,
    waitForItems,
} from "./support/api";

test.describe("feed pagination", () => {
    test("hidden when there's only one page", async ({ page }) => {
        await registerUser(page.request);
        await seedFeeds(page.request, [{ feedUrl: FIXTURE.rss }]);
        await ingestNow(page.request);
        await waitForItems(page.request, {
            predicate: (items) => items.length >= 2,
        });

        await page.goto("/");
        await expect(page.getByTestId("item-card")).toHaveCount(2);
        await expect(
            page.getByRole("navigation", { name: "Pagination" }),
        ).toBeHidden();
    });

    test("Next/previous navigate, update ?page=, and track aria-current", async ({
        page,
    }) => {
        await registerUser(page.request);
        // rss + atom + json = 6 items = 6 pages at page_size=1.
        await seedFeeds(page.request, [
            { feedUrl: FIXTURE.rss },
            { feedUrl: FIXTURE.atom },
            { feedUrl: FIXTURE.json },
        ]);
        await ingestNow(page.request);
        await waitForItems(page.request, {
            predicate: (items) =>
                items.some(
                    (i) =>
                        (i as { title?: string }).title ===
                        "E2E Fixture JSON Item One",
                ),
        });
        // Force a small page AFTER confirming all three feeds' items exist: page_size clamps
        // every /api/items response, including waitForItems' own polling GETs above.
        await setPageSize(page.request, 1);

        await page.goto("/");
        const nav = page.getByRole("navigation", { name: "Pagination" });
        await expect(nav).toBeVisible();
        await expect(
            nav.getByRole("button", { name: "1", exact: true }),
        ).toHaveAttribute("aria-current", "page");

        await page.getByRole("button", { name: "Next page" }).click();
        await expect(page).toHaveURL(/[?&]page=2/);
        await expect(
            nav.getByRole("button", { name: "2", exact: true }),
        ).toHaveAttribute("aria-current", "page");

        await page.getByRole("button", { name: "Previous page" }).click();
        await expect(page).not.toHaveURL(/[?&]page=/);
        await expect(
            nav.getByRole("button", { name: "1", exact: true }),
        ).toHaveAttribute("aria-current", "page");
    });

    test("ellipsis windowing renders for a large page count", async ({
        page,
    }) => {
        await registerUser(page.request);
        await seedFeeds(page.request, [
            { feedUrl: FIXTURE.rss },
            { feedUrl: FIXTURE.atom },
            { feedUrl: FIXTURE.json },
        ]);
        await ingestNow(page.request);
        await waitForItems(page.request, {
            predicate: (items) =>
                items.some(
                    (i) =>
                        (i as { title?: string }).title ===
                        "E2E Fixture JSON Item One",
                ),
        });
        // Force a small page AFTER confirming all three feeds' items exist: page_size clamps
        // every /api/items response, including waitForItems' own polling GETs above.
        await setPageSize(page.request, 1);

        await page.goto("/?page=3");
        const nav = page.getByRole("navigation", { name: "Pagination" });
        await expect(
            nav.getByRole("button", { name: "3", exact: true }),
        ).toHaveAttribute("aria-current", "page");
        // pageWindow(3, 6) = [1, 2, 3, 4, "…", 6] (Pagination.tsx pageWindow).
        await expect(nav.getByText("…")).toBeVisible();
        await expect(
            nav.getByRole("button", { name: "6", exact: true }),
        ).toBeVisible();
    });
});
