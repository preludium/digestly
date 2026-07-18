import { expect, test } from "@playwright/test";
import { ADMIN, expectToast, loginAs } from "./support/api";

// Tagged @serial (issue #43): this test flips `allow_registration`, an instance-wide gate that
// every `registerUser()` call in the whole suite depends on. Restoring allow_registration=true
// (the finally block below) is load-bearing: every later spec registers a user, and a
// left-closed toggle fails all of them. Keep the restore robust - do not add page navigation
// inside the try that could detach the switch. The "parallel" project's `dependencies: ["serial"]`
// (see playwright.config.ts) ensures this test - and the rest of the @serial project - always
// finishes before any parallel-project test starts, so no other spec can observe the toggle off.
test("admin manages the Open registration setting", { tag: "@serial" }, async ({
    page,
}) => {
    await loginAs(page.request, ADMIN.username, ADMIN.password);
    await page.goto("/admin/users");

    await expect(page.getByRole("heading", { name: "Users" })).toBeVisible();
    await expect(page.getByText("All accounts", { exact: true })).toBeVisible();
    await expect(page.getByRole("table")).toBeVisible();
    // The built-in admin appears in both the Username and Role ("admin") columns, so match the row
    // rather than a bare cell to avoid a strict-mode collision.
    await expect(
        page.getByRole("row").filter({ hasText: ADMIN.username }),
    ).toBeVisible();

    // The "Open registration" Switch has no aria-label (approved architecture), so it's scoped by
    // its card: CardTitle -> CardHeader -> Card.
    const registrationCard = page
        .getByText("Open registration", { exact: true })
        .locator("..")
        .locator("..");
    const registrationSwitch = registrationCard.getByRole("switch");
    await expect(registrationSwitch).toHaveAttribute("aria-checked", "true");

    try {
        await registrationSwitch.click();
        await expectToast(page, "Registration disabled");
        await expect(registrationSwitch).toHaveAttribute(
            "aria-checked",
            "false",
        );
    } finally {
        // CRITICAL for test independence: later specs register users through the API against this
        // shared, serial-run DB. Leaving registration closed fails every one of them, so restoring
        // it is the guaranteed final action - it runs even if an assertion above threw, and it
        // checks the live state rather than assuming the try block ran to completion.
        if (
            (await registrationSwitch.getAttribute("aria-checked")) !== "true"
        ) {
            await registrationSwitch.click();
        }
        await expect(registrationSwitch).toHaveAttribute(
            "aria-checked",
            "true",
        );
    }
});
