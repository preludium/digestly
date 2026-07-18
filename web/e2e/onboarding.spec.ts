import { expect, test } from "@playwright/test";
import { APP_URL, expectToast, registerUnonboarded } from "./support/api";

// The first-run onboarding overlay is what every other spec bypasses via markOnboarded (see
// support/api.ts). This file is the one place that leaves an account NOT onboarded, so it can
// assert the overlay itself: `registerUnonboarded` deliberately skips the markOnboarded step.
test.describe("onboarding overlay", () => {
    test.beforeEach(async ({ page }) => {
        await registerUnonboarded(page.request);
        await page.goto("/");
        await expect(
            page.getByRole("heading", { name: /Welcome to Digestly/ }),
        ).toBeVisible();
    });

    test("blocks interaction with the app behind it", async ({ page }) => {
        // The overlay is a `fixed inset-0` layer that intercepts pointer events - a nav link
        // sitting behind it is unclickable until the overlay is dismissed.
        await expect(
            page.getByRole("link", { name: "Manage" }).click({ timeout: 1000 }),
        ).rejects.toThrow();
        await expect(page).toHaveURL("/");
    });

    test("prefills the timezone field from the browser timezone", async ({
        page,
    }) => {
        const expectedTz = await page.evaluate(
            () => Intl.DateTimeFormat().resolvedOptions().timeZone,
        );
        await expect(page.getByLabel("Your timezone")).toHaveValue(expectedTz);
    });

    test("adds starter feeds and disables the button", async ({ page }) => {
        // The real starter-feed set points at public internet URLs (news.ycombinator.com,
        // reddit.com) and, once subscribed, the ingest scheduler would try to poll them - no spec
        // may reach the public internet, so the endpoint itself is mocked here. This covers the
        // UI contract (toast copy, button toggling to a disabled "Added" state); it does not
        // assert the real subscription count, which would require letting the scheduler run.
        await page.route("**/api/onboarding/starter-feeds", (route) =>
            route.fulfill({
                status: 200,
                contentType: "application/json",
                body: JSON.stringify({ added: 4 }),
            }),
        );
        await page.getByRole("button", { name: "Add starter feeds" }).click();
        await expectToast(page, "Added 4 starter feeds");
        const added = page.getByRole("button", { name: "Added" });
        await expect(added).toBeVisible();
        await expect(added).toBeDisabled();
    });

    test("Get started saves and closes, and does not return after reload", async ({
        page,
    }) => {
        // Delay the settings PUT so the pending "Saving…" state is observable.
        await page.route("**/api/settings", async (route) => {
            if (route.request().method() === "PUT") {
                await new Promise((resolve) => setTimeout(resolve, 300));
            }
            await route.continue();
        });
        await page.getByRole("button", { name: "Get started" }).click();
        await expect(
            page.getByRole("button", { name: "Saving…" }),
        ).toBeVisible();
        await expect(
            page.getByRole("heading", { name: /Welcome to Digestly/ }),
        ).toHaveCount(0);

        await page.reload();
        await expect(
            page.getByRole("heading", { name: /Welcome to Digestly/ }),
        ).toHaveCount(0);

        const settings = await page.request.get(`${APP_URL}/api/settings`);
        expect((await settings.json()).onboarded).toBe(true);
    });

    test("a save failure toasts and keeps the overlay open", async ({
        page,
    }) => {
        await page.route("**/api/settings", async (route) => {
            if (route.request().method() === "PUT") {
                await route.fulfill({
                    status: 500,
                    contentType: "application/json",
                    body: JSON.stringify({ error: "Could not save" }),
                });
                return;
            }
            await route.continue();
        });
        await page.getByRole("button", { name: "Get started" }).click();
        await expectToast(page, "Could not save");
        await expect(
            page.getByRole("heading", { name: /Welcome to Digestly/ }),
        ).toBeVisible();
    });
});
