// feed-preview.spec.ts — issue #45 (E2E: feed reading)
// ItemPreview.tsx + the URL-driven preview state in Feed.tsx: opening/closing via Escape, the
// Back button, and an overlay click; the deep-link cold-load skeleton; star/read persistence;
// "Open original" marking read; and sanitized content_html rendering.
import { expect, test } from "@playwright/test";
import {
    FIXTURE,
    itemIdByTitle,
    registerUser,
    seedFeedWithItems,
} from "./support/api";

test.describe("feed preview", () => {
    test.beforeEach(async ({ page }) => {
        await registerUser(page.request);
        await seedFeedWithItems(page.request);
    });

    test("Escape, the Back button, and an overlay click all close it", async ({
        page,
    }) => {
        await page.goto("/");
        const card = page
            .getByTestId("item-card")
            .filter({ hasText: FIXTURE.rssItemTitle });

        // Escape.
        await card.click();
        await expect(page).toHaveURL(/[?&]item=\d+/);
        await expect(page.getByRole("dialog")).toBeVisible();
        await page.keyboard.press("Escape");
        await expect(page).not.toHaveURL(/[?&]item=/);
        await expect(page.getByRole("dialog")).toBeHidden();

        // The Back button in the sheet's action bar.
        await card.click();
        await expect(page).toHaveURL(/[?&]item=\d+/);
        await page.getByRole("button", { name: "Back" }).click();
        await expect(page).not.toHaveURL(/[?&]item=/);
        await expect(page.getByRole("dialog")).toBeHidden();

        // An overlay click: the sheet panel is right-anchored and capped at sm:max-w-xl, and the
        // app's own sidebar nav occupies the far left, so click a point in the neutral band
        // between the two - guaranteed to be the overlay scrim, not app chrome or the panel.
        await card.click();
        await expect(page).toHaveURL(/[?&]item=\d+/);
        // Radix's dismissable layer ignores an outside pointerdown for a beat after opening (so
        // the very click that opens a dialog can't also immediately close it) - wait for the
        // dialog to actually be visible/settled before treating (500, 400) as an outside click.
        await expect(page.getByRole("dialog")).toBeVisible();
        await page.mouse.click(500, 400);
        await expect(page).not.toHaveURL(/[?&]item=/);
        await expect(page.getByRole("dialog")).toBeHidden();
    });

    test("deep-linking ?item=N on a cold load shows the skeleton then the item, and closes by dropping the param", async ({
        page,
    }) => {
        const itemId = await itemIdByTitle(page.request, FIXTURE.rssItemTitle);

        // Slow both the list and the detail fetch down so the loading skeleton is observable.
        // ItemPreview's `seed` can come from the ALREADY-loaded list cache (Feed.tsx: `items.data
        // ?.items.find(...)`), so slowing only the detail endpoint isn't enough - if the list
        // settles first, its cached row satisfies `view` and the skeleton branch never renders.
        // Both are otherwise same-machine SQLite reads that can resolve before a
        // screenshot-speed assertion lands.
        const delay = async (route: import("@playwright/test").Route) => {
            const response = await route.fetch();
            await new Promise((resolve) => setTimeout(resolve, 300));
            await route.fulfill({ response });
        };
        await page.route(`**/api/items/${itemId}`, delay);
        await page.route((url) => url.pathname === "/api/items", delay);

        await page.goto(`/?item=${itemId}`);

        const status = page.getByRole("status");
        await expect(status).toBeVisible();
        await expect(status.getByText("Loading article")).toBeAttached();

        await expect(
            page.getByRole("heading", { name: FIXTURE.rssItemTitle }),
        ).toBeVisible();
        await expect(status).toBeHidden();

        // This was a fresh navigation, not one openPreview() pushed - closing must drop the
        // param in place, not navigate(-1) out of the app.
        await page.getByRole("button", { name: "Back" }).click();
        expect(new URL(page.url()).searchParams.has("item")).toBe(false);
        expect(new URL(page.url()).origin).toBe("http://localhost:8099");
        await expect(page.getByTestId("item-card").first()).toBeVisible();
    });

    test("star/unstar and mark read/unread persist after reload", async ({
        page,
    }) => {
        const itemId = await itemIdByTitle(page.request, FIXTURE.rssItemTitle);
        await page.goto("/");
        await page
            .getByTestId("item-card")
            .filter({ hasText: FIXTURE.rssItemTitle })
            .click();

        const starWritten = page.waitForResponse(`**/api/items/${itemId}/star`);
        await page.getByRole("button", { name: "Star" }).click();
        await expect(
            page.getByRole("button", { name: "Unstar" }),
        ).toBeVisible();
        await starWritten;

        const readWritten = page.waitForResponse(`**/api/items/${itemId}/read`);
        await page.getByRole("button", { name: "Mark as read" }).click();
        await expect(
            page.getByRole("button", { name: "Mark as unread" }),
        ).toBeVisible();
        await readWritten;

        await page.reload();

        await expect(
            page.getByRole("button", { name: "Unstar" }),
        ).toBeVisible();
        await expect(
            page.getByRole("button", { name: "Mark as unread" }),
        ).toBeVisible();
    });

    test("Open original opens a new tab and marks the item read", async ({
        page,
        context,
    }) => {
        const itemId = await itemIdByTitle(page.request, FIXTURE.rssItemTitle);
        const detail = await page.request.get(`/api/items/${itemId}`);
        const { url } = (await detail.json()) as { url: string };

        // The fixture item's link points at a placeholder domain that is never actually served -
        // intercept it at the context level (covers the popup tab too) so the click never
        // reaches the network, real or otherwise.
        await context.route(url, (route) =>
            route.fulfill({
                status: 200,
                contentType: "text/html",
                body: "stub",
            }),
        );

        await page.goto("/");
        await page
            .getByTestId("item-card")
            .filter({ hasText: FIXTURE.rssItemTitle })
            .click();
        await expect(
            page.getByRole("button", { name: "Mark as read" }),
        ).toBeVisible();

        const readWritten = page.waitForResponse(`**/api/items/${itemId}/read`);
        const [popup] = await Promise.all([
            context.waitForEvent("page"),
            page.getByRole("link", { name: "Open original" }).click(),
        ]);
        await readWritten;
        await popup.close();

        await expect(
            page.getByRole("button", { name: "Mark as unread" }),
        ).toBeVisible();

        await page.reload();
        await expect(
            page.getByRole("button", { name: "Mark as unread" }),
        ).toBeVisible();
    });

    test("a reading item renders sanitized content_html", async ({ page }) => {
        await page.goto("/");
        await page
            .getByTestId("item-card")
            .filter({ hasText: FIXTURE.rssItemTitle })
            .click();
        // rssBody() (fixture-server.mjs) ships this exact paragraph as the item's description.
        await expect(
            page
                .locator(".article-content")
                .getByText("First fixture item body."),
        ).toBeVisible();
    });
});
