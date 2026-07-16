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
    //
    // STAGE-5 NOTE: if this assertion fails (list still empty), the offline grouped-items
    // fallback may not have produced content within the default lookback_days=1 window.
    // Fix: pass lookbackDays: 7 to adminDigestRun() above. Do not assume the endpoint
    // itself is broken before trying that parameter.
    await expect(page.getByText("No digests yet")).not.toBeVisible();

    // Click into the first digest detail. Rows are <a href="/digests/:id"> links; the
    // [href^="/digests/"] selector excludes the nav "Digests" link (/digests, no slash).
    await page.locator('a[href^="/digests/"]').first().click();

    // "Back to digests" must be visible on the detail page (DigestDetail.tsx:26-31).
    await expect(
        page.getByRole("link", { name: "Back to digests" }),
    ).toBeVisible();

    // The fixture feed title in the digest sections doubles as a serde drift check:
    // a rename of a DigestDetail payload field not mirrored in types.ts would cause
    // sections to render nothing and this assertion would fail.
    //
    // STAGE-5 NOTE: if this assertion fails but the detail page otherwise loads, the
    // digest may group by category (not feed), in which case switch to asserting
    // FIXTURE.rssItemTitle (the item title is present regardless of grouping).
    await expect(page.getByText(FIXTURE.rssFeedTitle)).toBeVisible();
});
