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

test("syncs System tabs with the URL and browser history", async ({ page }) => {
    await page.goto("/admin/system?tab=digest&view=compact");
    await expect(
        page.getByRole("heading", { name: "Digest", exact: true }),
    ).toBeVisible();

    await page.getByRole("button", { name: "AI", exact: true }).click();
    await expect(page).toHaveURL("/admin/system?tab=ai&view=compact");
    await expect(
        page.getByRole("heading", { name: "AI", exact: true }),
    ).toBeVisible();

    await page.getByRole("button", { name: "Digest", exact: true }).click();
    await page.goBack();
    await expect(
        page.getByRole("heading", { name: "AI", exact: true }),
    ).toBeVisible();
    await page.goForward();
    await expect(
        page.getByRole("heading", { name: "Digest", exact: true }),
    ).toBeVisible();

    await page.goto("/admin/system?tab=unknown");
    await expect(
        page.getByRole("heading", { name: "Ingestion", exact: true }),
    ).toBeVisible();
    await page.goto("/admin/system?view=compact");
    await expect(
        page.getByRole("heading", { name: "Ingestion", exact: true }),
    ).toBeVisible();
});

test("saves the YouTube video summaries switch as a partial AI setting", async ({
    page,
}) => {
    await page.getByRole("button", { name: "AI", exact: true }).click();
    const summaries = page.getByRole("switch", {
        name: "YouTube video summaries",
    });
    await expect(summaries).not.toBeChecked();

    const saveRequest = page.waitForRequest(
        (request) =>
            request.url().includes("/api/ai/settings") &&
            request.method() === "PUT",
    );
    const saveResponse = page.waitForResponse(
        (response) =>
            response.url().includes("/api/ai/settings") &&
            response.request().method() === "PUT",
    );
    await summaries.click();
    expect(JSON.parse((await saveRequest).postData() ?? "{}")).toEqual({
        youtube_auto_summary_enabled: true,
    });
    await saveResponse;
    await page.request.put("/api/ai/settings", {
        data: { youtube_auto_summary_enabled: false },
    });
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
