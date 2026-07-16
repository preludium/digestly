import { expect, test } from "@playwright/test";
import { ADMIN, loginAs } from "./support/api";

test("admin manages the Open registration setting", async ({ page }) => {
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
        await expect(page.getByText("Registration disabled")).toBeVisible();
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
