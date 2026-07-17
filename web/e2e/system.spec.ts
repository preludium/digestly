import { expect, test } from "@playwright/test";
import { ADMIN, loginAs } from "./support/api";

test.beforeEach(async ({ page }) => {
    await loginAs(page.request, ADMIN.username, ADMIN.password);
    await page.goto("/admin/system");
    await expect(page.getByRole("heading", { name: "System" })).toBeVisible();
});

test("switches System tabs and opens their Button actions", async ({
    page,
}) => {
    await expect(
        page.getByRole("heading", { name: "Ingestion", exact: true }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Delete", exact: true }).click();
    await expect(
        page.getByRole("alertdialog", { name: "Delete old items?" }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Cancel" }).click();

    await page.getByRole("button", { name: "AI", exact: true }).click();
    await expect(
        page.getByRole("heading", { name: "AI", exact: true }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Add provider" }).click();
    await expect(
        page.getByRole("dialog", { name: "Add an AI provider" }),
    ).toBeVisible();
    await page.keyboard.press("Escape");

    await page.getByRole("button", { name: "Digest", exact: true }).click();
    await expect(
        page.getByRole("heading", { name: "Digest", exact: true }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Run digest now" }).click();
    await expect(
        page.getByRole("alertdialog", { name: "Run digest now?" }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Cancel" }).click();
});

test("activates the waiting worker from the update banner", async ({
    page,
}) => {
    await page.evaluate(() => {
        Object.defineProperty(navigator, "serviceWorker", {
            configurable: true,
            value: {
                getRegistration: async () => ({
                    waiting: {
                        postMessage: (message: unknown) => {
                            document.body.dataset.workerMessage =
                                JSON.stringify(message);
                        },
                    },
                }),
                addEventListener: () => undefined,
            },
        });
        window.dispatchEvent(new CustomEvent("hf-sw-update"));
    });

    await page
        .getByRole("button", { name: /A new version is available/ })
        .click();
    await expect(page.locator("body")).toHaveAttribute(
        "data-worker-message",
        '{"type":"SKIP_WAITING"}',
    );
});
