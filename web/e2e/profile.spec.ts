import { request as apiRequest, expect, test } from "@playwright/test";
import {
    ADMIN,
    APP_URL,
    loginAs,
    registerUser,
    uniqueUsername,
} from "./support/api";

test("username change: validation, submit gating, and it persists", async ({
    page,
}) => {
    const { username } = await registerUser(page.request);
    await page.goto("/profile");

    const field = page.getByLabel("New username");
    const submit = page.getByRole("button", { name: "Change username" });
    await expect(field).toHaveValue(username);
    await expect(submit).toBeDisabled();

    await field.fill("ab");
    await field.press("Tab");
    await expect(page.getByText("3-32 characters")).toBeVisible();
    await expect(submit).toBeDisabled();

    const newName = uniqueUsername("renamed");
    await field.fill(newName);
    await expect(submit).toBeEnabled();
    await submit.click();
    await expect(page.locator("[data-sonner-toast]")).toContainText(
        "Username updated",
    );
    await expect(field).toHaveValue(newName);
    // Unchanged from the current value again -> disabled.
    await expect(submit).toBeDisabled();

    await page.reload();
    await expect(page.getByLabel("New username")).toHaveValue(newName);
});

test("password change: wrong current password errors, success clears fields, and the new password works", async ({
    page,
}) => {
    const { username, password } = await registerUser(page.request);
    await page.goto("/profile");

    await page.getByLabel("Current password").fill("totally-wrong-pw");
    await page
        .getByLabel("New password", { exact: true })
        .fill("brand-new-pw-1");
    await page.getByLabel("Confirm new password").fill("brand-new-pw-1");
    await page.getByRole("button", { name: "Change password" }).click();
    await expect(page.getByRole("alert")).toContainText(
        "current password is incorrect",
    );

    const newPassword = "brand-new-pw-1";
    await page.getByLabel("Current password").fill(password);
    await page.getByLabel("New password", { exact: true }).fill(newPassword);
    await page.getByLabel("Confirm new password").fill(newPassword);
    await page.getByRole("button", { name: "Change password" }).click();
    await expect(page.locator("[data-sonner-toast]")).toContainText(
        "Password changed",
    );
    await expect(page.getByLabel("Current password")).toHaveValue("");
    await expect(page.getByLabel("New password", { exact: true })).toHaveValue(
        "",
    );
    await expect(page.getByLabel("Confirm new password")).toHaveValue("");

    await page.getByRole("button", { name: "Account menu" }).click();
    await page.getByRole("menuitem", { name: "Log out" }).click();
    await expect(page).toHaveURL("/login");

    await page.getByLabel("Username").fill(username);
    await page.getByLabel("Password", { exact: true }).fill(newPassword);
    await page.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(page).toHaveURL("/");
});

test("log out everywhere invalidates a second, pre-existing session", async ({
    page,
}) => {
    const { username, password } = await registerUser(page.request);

    // A second identity/session for the same user, in its own cookie jar.
    const second = await apiRequest.newContext({ baseURL: APP_URL });
    await loginAs(second, username, password);
    const before = await second.get(`${APP_URL}/api/me`);
    expect(before.ok()).toBe(true);

    await page.goto("/profile");
    await page.getByRole("button", { name: "Log out everywhere" }).click();
    await expect(page).toHaveURL("/login");

    const after = await second.get(`${APP_URL}/api/me`);
    expect(after.status()).toBe(401);
    await second.dispose();
});

test("delete my account: confirm dialog, then the credentials no longer work", async ({
    page,
}) => {
    const { username, password } = await registerUser(page.request);
    await page.goto("/profile");

    await page.getByRole("button", { name: "Delete my account" }).click();
    const dialog = page.getByRole("alertdialog", {
        name: "Delete your account?",
    });
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Delete my account" }).click();
    await expect(page).toHaveURL("/login");

    const ctx = await apiRequest.newContext({ baseURL: APP_URL });
    const attempt = await ctx.post(`${APP_URL}/api/auth/login`, {
        data: { username, password },
    });
    expect(attempt.ok()).toBe(false);
    await ctx.dispose();
});

test("the built-in admin hides the username section and the danger zone", async ({
    page,
}) => {
    await loginAs(page.request, ADMIN.username, ADMIN.password);
    await page.goto("/profile");

    // Sanity anchor - Password/Session sections are always shown, so the page did load.
    await expect(
        page.getByRole("heading", { name: "Password", level: 3 }),
    ).toBeVisible();

    await expect(
        page.getByRole("heading", { name: "Username", level: 3 }),
    ).toHaveCount(0);
    await expect(page.getByLabel("New username")).toHaveCount(0);

    await expect(
        page.getByRole("heading", { name: "Danger zone", level: 3 }),
    ).toHaveCount(0);
    await expect(
        page.getByRole("button", { name: "Delete my account" }),
    ).toHaveCount(0);
});
