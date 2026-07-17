import { expect, test } from "@playwright/test";
import {
    adminDigestRun,
    FIXTURE,
    registerUser,
    seedFeedWithItems,
} from "./support/api";

test("admin digest run produces a viewable digest", async ({ page }) => {
    // Register a fresh user; page.request shares the browser context's cookie jar,
    // so page.goto() will be authenticated without storageState.
    await registerUser(page.request);

    // Subscribe the fixture RSS feed, trigger ingest, and poll until items appear.
    // The fixture server injects "now" timestamps so items survive the default
    // max_item_age_days=1 cutoff (backend/src/ingest/store.rs:49,81-83).
    await seedFeedWithItems(page.request);

    // Run the admin digest in a SEPARATE request context so the admin hf_session
    // never clobbers the page user's session (see cookie contract in support/api.ts).
    // With no AI provider configured the backend falls back to raw grouped items
    // (digest/mod.rs:195-198,308-331), which produces a non-empty archived digest.
    await adminDigestRun();

    // View the digest list as the page user.
    await page.goto("/digests");
    await expect(page.getByRole("heading", { name: "Digests" })).toBeVisible();

    // Assert at least one digest row was generated, not the "No digests yet" empty state.
    await expect(page.getByText("No digests yet")).not.toBeVisible();

    // Click into the first digest detail. Rows are <a href="/digests/:id"> links; the
    // [href^="/digests/"] selector excludes the nav "Digests" link (/digests, no slash).
    await page.locator('a[href^="/digests/"]').first().click();

    // "Back to digests" must be visible on the detail page (DigestDetail.tsx:26-31).
    await expect(
        page.getByRole("link", { name: "Back to digests" }),
    ).toBeVisible();

    // The fixture feed title renders as a section header on the detail page, so this
    // catches DigestDetail payload drift (a renamed section field would leave the
    // sections empty). ItemDto.title drift is covered by items.spec / feeds.spec.
    await expect(page.getByText(FIXTURE.rssFeedTitle)).toBeVisible();
});
