// feeds.spec.ts — SLICE 3 (specs-feeds)
// Drives the add-feed → ingest → edit-category → unsubscribe flow through the real UI.
// Uses a unique user per run so tests are order-independent and avoid the 60 s per-user
// ingest cooldown (uniqueUsername in registerUser).
import { expect, test } from "@playwright/test";
import { APP_URL, FIXTURE, registerUser } from "./support/api";

test("add, ingest, edit category, and unsubscribe a feed", async ({ page }) => {
    // Register a fresh user; page.request shares the browser-context cookie jar with page,
    // so page.goto() is already authenticated after this call — no storageState needed.
    await registerUser(page.request);

    // Create a second category so the edit-category step has something to change to.
    // Fresh users only get one built-in category: "Other" (backend/src/seed.rs DEFAULT_CATEGORIES).
    await page.request.post(`${APP_URL}/api/categories`, {
        data: { name: "Tech News" },
    });

    // ── Add feed via "Use this URL as a feed directly" fallback ──────────────────────────────
    await page.goto("/manage");

    // The header "Add feed" button is always present on /manage. Use exact:true to distinguish
    // it from per-category "Add feed to <cat>" aria-label buttons that appear once feeds exist.
    await page.getByRole("button", { name: "Add feed", exact: true }).click();

    const addDialog = page.getByRole("dialog", { name: "Add a feed" });
    await expect(addDialog).toBeVisible();

    await addDialog.getByPlaceholder("Feed or site URL").fill(FIXTURE.rss);

    // "Find" triggers POST /api/feeds/discover. For localhost URLs, discover.rs calls
    // guard_public_url which rejects loopback and returns Ok(vec![]) — not an error — so
    // candidates becomes [] and the UI shows the "Use this URL as a feed directly" fallback
    // (AddFeedModal.tsx:130-137 renders when candidates.length === 0 and !selected).
    await addDialog.getByRole("button", { name: "Find" }).click();
    await addDialog
        .getByRole("button", { name: "Use this URL as a feed directly" })
        .click();

    // ConfigureStep: select the first (only) category.
    // SelectTrigger has id="category". Radix SelectContent renders in a portal outside the
    // dialog DOM tree, so options must be queried via page scope, not addDialog.
    await addDialog.locator("#category").click();
    await page.getByRole("option").first().click();

    await addDialog.getByRole("button", { name: "Add feed" }).click();
    await expect(page.getByText("Feed added")).toBeVisible();

    // ── Click "Ingest now" and assert the fixture item appears ───────────────────────────────
    await page.goto("/");
    await page.getByRole("button", { name: "Ingest now" }).click();

    // The backend has a per-host 1 500 ms politeness delay (scheduler.rs:27).
    // Use a generous timeout so CI on a slow runner still passes.
    await expect(page.getByText(FIXTURE.rssItemTitle)).toBeVisible({
        timeout: 30_000,
    });

    // ── Edit the feed's category ─────────────────────────────────────────────────────────────
    await page.goto("/manage");

    // Feed title is populated from the RSS <title> element during ingest.
    const feedRow = page
        .getByRole("listitem")
        .filter({ hasText: FIXTURE.rssFeedTitle });

    await feedRow.getByRole("button", { name: "Actions" }).click();
    await page.getByRole("menuitem", { name: "Edit" }).click();

    const editDialog = page.getByRole("dialog", { name: "Edit feed" });
    await expect(editDialog).toBeVisible();

    // FeedEditModal category SelectTrigger has id="cat". Portal options: use page scope.
    await editDialog.locator("#cat").click();
    await page.getByRole("option", { name: "Tech News" }).click();

    await editDialog.getByRole("button", { name: "Save" }).click();
    await expect(page.getByText("Feed updated")).toBeVisible();

    // ── Unsubscribe ─────────────────────────────────────────────────────────────────────────
    // Edit dialog closes; all CategorySection Collapsibles start open (useState(true) in
    // CategoryCard), so the row is visible even after moving to the new "Tech News" section.
    await feedRow.getByRole("button", { name: "Actions" }).click();
    await page.getByRole("menuitem", { name: "Unsubscribe" }).click();

    // ConfirmDialog wraps AlertDialog → the root element has role="alertdialog".
    const confirmDialog = page.getByRole("alertdialog");
    await expect(confirmDialog).toBeVisible();
    await confirmDialog.getByRole("button", { name: "Unsubscribe" }).click();

    await expect(page.getByText("Unsubscribed")).toBeVisible();
    // useUnsubscribe invalidates the feeds query; after refetch the <li> is gone.
    await expect(feedRow).toHaveCount(0);
});
