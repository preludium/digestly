import { expect, test } from "@playwright/test";
import { registerUser, uniqueUsername } from "./support/api";

test("registers through the UI, logs out, and logs back in", async ({
    page,
}) => {
    const username = uniqueUsername();
    const password = "e2e-password-1";

    await page.goto("/register");
    await page.getByLabel("Username").fill(username);
    await page.getByLabel("Password", { exact: true }).fill(password);
    await page.getByLabel("Confirm password").fill(password);
    await page.getByRole("button", { name: "Create account" }).click();
    await expect(page).toHaveURL("/");

    // A brand-new account lands on the first-run onboarding overlay (a full-screen layer that
    // intercepts pointer events). Dismiss it the way a real user would before touching the app.
    await page.getByRole("button", { name: "Get started" }).click();

    await page.getByRole("button", { name: "Account menu" }).click();
    await page.getByRole("menuitem", { name: "Log out" }).click();
    await expect(page).toHaveURL("/login");

    await page.getByLabel("Username").fill(username);
    await page.getByLabel("Password", { exact: true }).fill(password);
    // "Sign in" is exact because the passkey button is also labelled "Sign in with a passkey".
    await page.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(page).toHaveURL("/");
});

test("shows an error banner for a bad password", async ({ page }) => {
    // Seed the user through the API - this test asserts the failed-login banner, not registration.
    const { username, password } = await registerUser(page.request);
    await page.goto("/");
    await expect(page).toHaveURL("/");

    await page.getByRole("button", { name: "Account menu" }).click();
    await page.getByRole("menuitem", { name: "Log out" }).click();
    await expect(page).toHaveURL("/login");

    await page.getByLabel("Username").fill(username);
    await page
        .getByLabel("Password", { exact: true })
        .fill(`${password}-wrong`);
    await page.getByRole("button", { name: "Sign in", exact: true }).click();

    await expect(page.getByText("Invalid username or password")).toBeVisible();
    await expect(page).toHaveURL("/login");
});
