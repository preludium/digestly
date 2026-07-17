import { expect, test } from "@playwright/test";
import { ADMIN, loginAs } from "../support/api";

test("captures shared primitive screens", async ({ page }) => {
    const capture = (name: string) =>
        page.screenshot({
            path: `ui-screenshots/${name}.png`,
            fullPage: true,
        });

    await loginAs(page.request, ADMIN.username, ADMIN.password);

    await page.goto("/settings");
    await page.getByRole("button", { name: "General" }).click();
    await expect(page.getByRole("heading", { name: "General" })).toBeVisible();
    await capture("settings-general");

    await page.goto("/admin/system");
    await expect(
        page.getByRole("heading", { name: "Ingestion", exact: true }),
    ).toBeVisible();
    await capture("system-ingestion");

    await page.getByRole("button", { name: "AI", exact: true }).click();
    await page.getByRole("button", { name: "Add provider" }).click();
    await expect(
        page.getByRole("dialog", { name: "Add an AI provider" }),
    ).toBeVisible();
    await capture("system-ai-dialog");

    await page.getByRole("button", { name: "Close" }).click();
    await page.evaluate(() => {
        Object.defineProperty(navigator, "serviceWorker", {
            configurable: true,
            value: {
                getRegistration: async () => ({ waiting: null }),
                addEventListener: () => undefined,
            },
        });
        window.dispatchEvent(new CustomEvent("hf-sw-update"));
    });
    await expect(
        page.getByRole("button", { name: /A new version is available/ }),
    ).toBeVisible();
    await capture("update-banner");
});
