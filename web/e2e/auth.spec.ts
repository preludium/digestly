import { request as apiRequest, expect, test } from "@playwright/test";
import {
    APP_URL,
    registerUser,
    seedAdminSetting,
    uniqueUsername,
    withAdmin,
} from "./support/api";

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

test("register field validation gates the submit button", async ({ page }) => {
    await page.goto("/register");
    const submit = page.getByRole("button", { name: "Create account" });
    await expect(submit).toBeDisabled();

    await page.getByLabel("Username").fill("ab");
    await page.getByLabel("Username").press("Tab");
    await expect(page.getByText("At least 3 characters")).toBeVisible();
    await expect(submit).toBeDisabled();

    await page.getByLabel("Username").fill(uniqueUsername());
    await page.getByLabel("Password", { exact: true }).fill("short1");
    await page.getByLabel("Password", { exact: true }).press("Tab");
    await expect(page.getByText("At least 8 characters")).toBeVisible();
    await expect(submit).toBeDisabled();

    await page.getByLabel("Password", { exact: true }).fill("longenough1");
    await page.getByLabel("Confirm password").fill("mismatched1");
    await page.getByLabel("Confirm password").press("Tab");
    await expect(page.getByText("Passwords do not match")).toBeVisible();
    await expect(submit).toBeDisabled();

    await page.getByLabel("Confirm password").fill("longenough1");
    await expect(submit).toBeEnabled();
});

test("duplicate username surfaces the server error in an ErrorBanner", async ({
    page,
}) => {
    // Seed the "taken" username through a SEPARATE context so it never logs the page itself in -
    // this test drives the register form on a logged-out page.
    const ctx = await apiRequest.newContext({ baseURL: APP_URL });
    const { username } = await registerUser(ctx);
    await ctx.dispose();

    await page.goto("/register");
    await page.getByLabel("Username").fill(username);
    await page.getByLabel("Password", { exact: true }).fill("another-pw-1");
    await page.getByLabel("Confirm password").fill("another-pw-1");
    await page.getByRole("button", { name: "Create account" }).click();

    await expect(page.getByRole("alert")).toContainText(
        "username already taken",
    );
    await expect(page).toHaveURL("/register");
});

test("login has blur-triggered validation and a pending button state", async ({
    page,
}) => {
    // Register on a separate context so this page stays logged out for the /login assertions.
    const ctx = await apiRequest.newContext({ baseURL: APP_URL });
    const { username, password } = await registerUser(ctx);
    await ctx.dispose();

    await page.goto("/login");
    await page.getByLabel("Username").click();
    await page.getByLabel("Username").press("Tab");
    await expect(page.getByText("Username is required")).toBeVisible();

    await page.getByLabel("Password", { exact: true }).click();
    await page.getByLabel("Password", { exact: true }).press("Tab");
    await expect(page.getByText("Password is required")).toBeVisible();

    // Delay the login response so the pending "Signing in…" state is observable.
    await page.route("**/api/auth/login", async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 300));
        await route.continue();
    });
    await page.getByLabel("Username").fill(username);
    await page.getByLabel("Password", { exact: true }).fill(password);
    await page.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(
        page.getByRole("button", { name: "Signing in…" }),
    ).toBeVisible();
    await expect(page).toHaveURL("/");
});

test("redirects: logged-in vs logged-out routes, and an unknown route", async ({
    page,
}) => {
    // Logged-out visiting an app route -> /login
    await page.goto("/manage");
    await expect(page).toHaveURL("/login");

    // Logged-in visiting /login or /register -> /
    await registerUser(page.request);
    await page.goto("/login");
    await expect(page).toHaveURL("/");
    await page.goto("/register");
    await expect(page).toHaveURL("/");

    // Unknown route -> NotFound, which sits outside AppShell (no sidebar).
    await page.goto("/this-route-does-not-exist");
    await expect(page.getByRole("heading", { name: "404" })).toBeVisible();
    await expect(page.locator('[data-slot="sidebar"]')).toHaveCount(0);
});

// Tagged @serial (issue #43/#44): flips the instance-wide `allow_registration` gate that every
// `registerUser()` call in the whole suite depends on. Restoring it in afterAll is load-bearing -
// see admin.spec.ts for the same contract. The "parallel" project's `dependencies: ["serial"]`
// (playwright.config.ts) ensures this always finishes before any parallel-project test starts.
test.describe("registration disabled", { tag: "@serial" }, () => {
    test.beforeAll(async () => {
        await withAdmin((admin) =>
            seedAdminSetting(admin, { allow_registration: false }),
        );
    });

    test.afterAll(async () => {
        await withAdmin((admin) =>
            seedAdminSetting(admin, { allow_registration: true }),
        );
    });

    test("hides the Register route and the Login page's Register link", async ({
        page,
    }) => {
        await page.goto("/register");
        await expect(
            page.getByText("Registration is disabled", { exact: true }),
        ).toBeVisible();
        await expect(
            page.getByRole("link", { name: "Back to sign in" }),
        ).toBeVisible();

        await page.goto("/login");
        await expect(page.getByRole("link", { name: "Register" })).toHaveCount(
            0,
        );
    });
});
