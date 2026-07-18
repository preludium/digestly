// health.spec.ts
// `Health.tsx` renders every row twice: `HealthCard` (an <li>) below the `sm` breakpoint and
// `HealthRow` (a <tr>) at/above it. Both are mounted in the DOM at every viewport width - only a
// CSS `hidden`/`sm:block` toggle decides which one is *visible*. An unscoped selector (e.g.
// `page.getByText(feedTitle)`) therefore matches both and throws a Playwright strict-mode
// violation. `data-testid="health-row-table"` / `"health-row-card"` (issue #42) let a spec target
// exactly one breakpoint's row.
import { expect, test } from "@playwright/test";
import { registerUser, seedFailingFeed } from "./support/api";

test("feed health shows a failing feed, scoped to one breakpoint's row", async ({
    page,
}) => {
    await registerUser(page.request);
    const { feedId } = await seedFailingFeed(page.request);

    await page.goto("/health");
    await expect(
        page.getByRole("heading", { name: "Feed health" }),
    ).toBeVisible();

    // Desktop Chrome's default viewport (1280x720, see playwright.config.ts devices) is above the
    // `sm` breakpoint, so the table is the visible row and the card is CSS-hidden - but both exist
    // in the DOM for this feed. Scoping by testid + feed id resolves to exactly one element; a
    // bare text/role selector here would resolve to both and fail strict mode.
    const desktopRow = page.locator(
        `[data-testid="health-row-table"][data-feed-id="${feedId}"]`,
    );
    await expect(desktopRow).toBeVisible();
    await expect(desktopRow).toContainText("Failing");

    // The mobile card for the same feed is present (proving the duplicate-render hazard is real
    // at this viewport), just not visible.
    const mobileRow = page.locator(
        `[data-testid="health-row-card"][data-feed-id="${feedId}"]`,
    );
    await expect(mobileRow).toHaveCount(1);
    await expect(mobileRow).toBeHidden();
});
