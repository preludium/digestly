// feed-filters.spec.ts — issue #45 (E2E: feed reading)
// FilterBar.tsx: topic chips (count + ordering + toggle), the Type/Status/When refine pill, the
// sort pill, the Clear affordance's activeFilterCount rule, and the two empty-filter surfaces.
import { expect, test } from "@playwright/test";
import {
    FIXTURE,
    firstCategoryId,
    ingestNow,
    itemIdByTitle,
    registerUser,
    seedCategory,
    seedFeeds,
    seedFeedsSequentially,
    seedItems,
    setPageSize,
    waitForItems,
} from "./support/api";

/** The desktop refine pill's three Selects, in DOM order (FilterBar.tsx:219-269). */
async function selectRefine(
    page: import("@playwright/test").Page,
    index: 0 | 1 | 2,
    optionName: string,
) {
    await page
        .locator(
            '[data-testid="refine-pill"][data-variant="desktop"] [role="combobox"]',
        )
        .nth(index)
        .click();
    await page.getByRole("option", { name: optionName }).click();
}

async function cardTitles(page: import("@playwright/test").Page) {
    return page
        .getByTestId("item-card")
        .locator("h3")
        .evaluateAll((els) => els.map((el) => el.textContent?.trim() ?? ""));
}

/** Poll until the grid's first two / last two cards match these (order-within-group-agnostic)
 *  title sets. `useItems` keeps the previous page's data visible while a re-sorted page loads
 *  (`keepPreviousData`), so reading `cardTitles` immediately after a sort change can catch the
 *  stale order - `toPass` retries until the refetch actually lands. */
async function expectOrder(
    page: import("@playwright/test").Page,
    firstTwo: string[],
    lastTwo: string[],
) {
    await expect(async () => {
        const titles = await cardTitles(page);
        expect(titles.slice(0, 2).sort()).toEqual([...firstTwo].sort());
        expect(titles.slice(2, 4).sort()).toEqual([...lastTwo].sort());
    }).toPass();
}

test.describe("feed filters", () => {
    test("topic chips: counts, desc ordering, and toggle back to all", async ({
        page,
    }) => {
        await registerUser(page.request);
        const catA = await firstCategoryId(page.request);
        const catB = await seedCategory(page.request, "E2E Topic B");
        // catA gets 4 items (rss + atom), catB gets 2 (json) - distinct counts to assert
        // desc ordering unambiguously.
        await seedFeeds(page.request, [
            { feedUrl: FIXTURE.rss, categoryId: catA },
            { feedUrl: FIXTURE.atom, categoryId: catA },
            { feedUrl: FIXTURE.json, categoryId: catB },
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
        await page.goto("/");

        const chips = page.locator('[data-testid="topic-chip"]');
        const chipAll = page.locator(
            '[data-testid="topic-chip"][data-category-id="all"]',
        );
        const chipA = page.locator(
            `[data-testid="topic-chip"][data-category-id="${catA}"]`,
        );
        const chipB = page.locator(
            `[data-testid="topic-chip"][data-category-id="${catB}"]`,
        );

        await expect(chipAll).toContainText("6");
        await expect(chipA).toContainText("4");
        await expect(chipB).toContainText("2");

        // Ordering is by count desc (FilterBar.tsx:148-150): catA (4) before catB (2).
        const order = await chips.evaluateAll((els) =>
            els.map((el) => el.getAttribute("data-category-id")),
        );
        expect(order[0]).toBe("all");
        expect(order.indexOf(String(catA))).toBeLessThan(
            order.indexOf(String(catB)),
        );

        // Selecting catB narrows to its 2 items only.
        await chipB.click();
        await expect(page).toHaveURL(new RegExp(`[?&]cat=${catB}(&|$)`));
        await expect(page.getByTestId("item-card")).toHaveCount(2);

        // Clicking the already-active chip toggles back to "all" (FilterBar.tsx:181-183).
        await chipB.click();
        await expect(page).not.toHaveURL(/[?&]cat=/);
        await expect(page.getByTestId("item-card")).toHaveCount(6);
    });

    test("type, status, and when each narrow the list correctly", async ({
        page,
    }) => {
        await registerUser(page.request);
        // Both feeds in one seedFeeds + one ingestNow: a second refresh-all for the same user
        // within 60s 429s (events.rs::COOLDOWN), so seedYoutubeFeed's own internal ingestNow
        // can't follow a prior one on this session.
        await seedFeeds(page.request, [
            { feedUrl: FIXTURE.rss },
            { feedUrl: FIXTURE.youtube, kind: "youtube" },
        ]);
        await ingestNow(page.request);
        await waitForItems(page.request, {
            predicate: (items) =>
                items.length >= 2 &&
                items.some(
                    (i) =>
                        (i as { title?: string }).title ===
                        "E2E YouTube Fixture Video",
                ),
        });
        const videoId = await itemIdByTitle(
            page.request,
            "E2E YouTube Fixture Video",
        );

        const readItemId = await itemIdByTitle(
            page.request,
            FIXTURE.rssItemTitle,
        );
        await seedItems(page.request, [
            { itemId: readItemId, isRead: true },
            { itemId: videoId, isStarred: true },
        ]);

        await page.goto("/");
        await expect(page.getByTestId("item-card")).toHaveCount(3);

        // --- Type ---
        await selectRefine(page, 0, "🎬 Videos");
        await expect(page).toHaveURL(/[?&]type=video/);
        await expect(page.getByTestId("item-card")).toHaveCount(1);
        await expect(page.getByText(FIXTURE.rssItemTitle)).toBeHidden();

        await selectRefine(page, 0, "📖 Reading");
        await expect(page).toHaveURL(/[?&]type=reading/);
        await expect(page.getByTestId("item-card")).toHaveCount(2);

        await selectRefine(page, 0, "All");
        await expect(page).not.toHaveURL(/[?&]type=/);
        await expect(page.getByTestId("item-card")).toHaveCount(3);

        // --- Status ---
        await selectRefine(page, 1, "Unread");
        await expect(page).toHaveURL(/[?&]status=unread/);
        await expect(page.getByTestId("item-card")).toHaveCount(2);
        await expect(page.getByText(FIXTURE.rssItemTitle)).toBeHidden();

        await selectRefine(page, 1, "★ Starred");
        await expect(page).toHaveURL(/[?&]status=starred/);
        await expect(page.getByTestId("item-card")).toHaveCount(1);

        await selectRefine(page, 1, "All");
        await expect(page.getByTestId("item-card")).toHaveCount(3);

        // --- When ---
        await selectRefine(page, 2, "Yesterday");
        await expect(page).toHaveURL(/[?&]when=yesterday/);
        await expect(page.getByTestId("item-card")).toHaveCount(0);
        await expect(
            page.getByText("Nothing matches these filters"),
        ).toBeVisible();

        await selectRefine(page, 2, "Today");
        await expect(page).toHaveURL(/[?&]when=today/);
        await expect(page.getByTestId("item-card")).toHaveCount(3);

        await selectRefine(page, 2, "This week");
        await expect(page).toHaveURL(/[?&]when=week/);
        await expect(page.getByTestId("item-card")).toHaveCount(3);

        await selectRefine(page, 2, "All time");
        await expect(page).toHaveURL(/[?&]when=all/);
        await expect(page.getByTestId("item-card")).toHaveCount(3);

        // Only "Last 24 hours" (the default) is omitted from the URL.
        await selectRefine(page, 2, "Last 24 hours");
        await expect(page).not.toHaveURL(/[?&]when=/);
        await expect(page.getByTestId("item-card")).toHaveCount(3);
    });

    test("Clear only appears once a facet is active, ignoring sort/page/q", async ({
        page,
    }) => {
        await registerUser(page.request);
        await seedFeeds(page.request, [{ feedUrl: FIXTURE.rss }]);
        await ingestNow(page.request);
        await waitForItems(page.request, {
            predicate: (items) => items.length >= 2,
        });
        // Force a small page AFTER confirming both items exist: page_size clamps every
        // /api/items response (including this helper's own polling GETs), so setting it first
        // would make waitForItems' length>=2 check unsatisfiable.
        await setPageSize(page.request, 1);
        await page.goto("/");

        const clear = page.getByRole("button", { name: "Clear" });
        await expect(clear).toBeHidden();

        // Sort doesn't count toward activeFilterCount (useFeedFilters.ts FACET_KEYS).
        await page
            .locator('[data-testid="sort-pill"] [role="combobox"]')
            .click();
        await page.getByRole("option", { name: "Oldest" }).click();
        await expect(clear).toBeHidden();

        // Nor does q.
        await page.getByLabel("Search articles").fill(FIXTURE.rssItemTitle);
        await expect(clear).toBeHidden();
        await page.getByLabel("Search articles").clear();

        // Nor does page (page_size=1 gives 2 pages here).
        await page.getByRole("button", { name: "Next page" }).click();
        await expect(page).toHaveURL(/[?&]page=2/);
        await expect(clear).toBeHidden();

        // A facet does count.
        await selectRefine(page, 1, "Unread");
        await expect(clear).toBeVisible();

        await clear.click();
        await expect(clear).toBeHidden();
        await expect(page).not.toHaveURL(/[?&]status=/);
    });

    test("sort modes reorder the list (asserts order for new/old/unread)", async ({
        page,
    }) => {
        await registerUser(page.request);
        // Ingested with a >1s gap so atom's items are strictly newer than rss's.
        await seedFeedsSequentially(page.request, [
            { feedUrl: FIXTURE.rss },
            { feedUrl: FIXTURE.atom },
        ]);
        await page.goto("/");
        await expect(page.getByTestId("item-card")).toHaveCount(4);

        const rssTitles = [FIXTURE.rssItemTitle, "E2E Fixture RSS Item Two"];
        const atomTitles = [
            "E2E Fixture Atom Item One",
            "E2E Fixture Atom Item Two",
        ];

        // Newest (default) - atom (later ingest) first, rss last.
        await expectOrder(page, atomTitles, rssTitles);

        // Oldest - reversed.
        await page
            .locator('[data-testid="sort-pill"] [role="combobox"]')
            .click();
        await page.getByRole("option", { name: "Oldest" }).click();
        await expect(page).toHaveURL(/[?&]sort=old/);
        await expectOrder(page, rssTitles, atomTitles);

        // Unread first - mark every rss item read; atom (unread) must sort above rss (read)
        // regardless of published_at.
        const rssOneId = await itemIdByTitle(
            page.request,
            FIXTURE.rssItemTitle,
        );
        const rssTwoId = await itemIdByTitle(
            page.request,
            "E2E Fixture RSS Item Two",
        );
        await seedItems(page.request, [
            { itemId: rssOneId, isRead: true },
            { itemId: rssTwoId, isRead: true },
        ]);
        await page
            .locator('[data-testid="sort-pill"] [role="combobox"]')
            .click();
        await page.getByRole("option", { name: "Unread first" }).click();
        await expect(page).toHaveURL(/[?&]sort=unread/);
        await expectOrder(page, atomTitles, rssTitles);

        // The remaining three modes don't have fixture data to order by (score/comments/reading
        // time all tie for rss/atom fixture items - see KNOWN BACKEND LIMITATIONS), so just
        // exercise the selection: the URL updates and the list keeps rendering.
        for (const [label, value] of [
            ["Quickest read", "quick"],
            ["Most popular", "top"],
            ["Most discussed", "discussed"],
        ] as const) {
            await page
                .locator('[data-testid="sort-pill"] [role="combobox"]')
                .click();
            await page.getByRole("option", { name: label }).click();
            await expect(page).toHaveURL(new RegExp(`[?&]sort=${value}`));
            await expect(page.getByTestId("item-card")).toHaveCount(4);
        }
    });

    test("the live count matches the rendered rows", async ({ page }) => {
        await registerUser(page.request);
        await seedFeeds(page.request, [{ feedUrl: FIXTURE.rss }]);
        await ingestNow(page.request);
        await waitForItems(page.request, {
            predicate: (items) => items.length >= 2,
        });
        await page.goto("/");

        const resultLine = page.locator("p").filter({ hasText: "articles" });
        await expect(resultLine).toHaveText("2 articles");
        await expect(page.getByTestId("item-card")).toHaveCount(2);

        await selectRefine(page, 0, "🎬 Videos");
        await expect(resultLine).toHaveText("0 articles");
        await expect(page.getByTestId("item-card")).toHaveCount(0);
    });
});
