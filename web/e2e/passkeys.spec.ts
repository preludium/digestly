import { expect, type Page, test } from "@playwright/test";
import { expectToast, registerUser } from "./support/api";

// CDP's WebAuthn domain (virtual authenticator) is only available on Chromium.
test.skip(
    ({ browserName }) => browserName !== "chromium",
    "CDP/WebAuthn is Chromium-only",
);

/** Wires up a CDP virtual authenticator on `page` - the CDP/WebAuthn boilerplate shared by every
 *  test in this file. Resident keys + automatic presence simulation, so ceremonies resolve
 *  without any UI prompt.
 *
 *  `transport` defaults to "internal" (a platform authenticator, e.g. Touch ID). Chrome only
 *  allows one "internal" virtual authenticator per browser environment, so a test that needs a
 *  *second* concurrent authenticator (to enroll a second passkey for the same user) must pass a
 *  non-internal transport such as "usb" - see CDP error "Chrome only supports one internal
 *  authenticator per environment". */
async function addVirtualAuthenticator(
    page: Page,
    transport: "internal" | "usb" = "internal",
) {
    const client = await page.context().newCDPSession(page);
    await client.send("WebAuthn.enable", { enableUI: false });
    const { authenticatorId } = await client.send(
        "WebAuthn.addVirtualAuthenticator",
        {
            options: {
                protocol: "ctap2",
                transport,
                hasResidentKey: true,
                hasUserVerification: true,
                isUserVerified: true,
                automaticPresenceSimulation: true,
            },
        },
    );
    return { client, authenticatorId };
}

async function addPasskey(page: Page, name: string) {
    await page.getByRole("button", { name: "Add a passkey" }).click();
    const dialog = page.getByRole("dialog", { name: "Add a passkey" });
    await dialog.getByLabel("Passkey name").fill(name);
    await dialog.getByRole("button", { name: "Continue" }).click();
}

test("adds a passkey and signs in with it", async ({ page }) => {
    const { username } = await registerUser(page.request);
    await page.goto("/");
    await expect(page).toHaveURL("/");

    const { client, authenticatorId } = await addVirtualAuthenticator(page);

    await page.goto("/profile");
    await addPasskey(page, "E2E passkey");

    await expect(page.getByText("E2E passkey")).toBeVisible();
    const { credentials } = await client.send("WebAuthn.getCredentials", {
        authenticatorId,
    });
    expect(credentials).toHaveLength(1);

    await page.getByRole("button", { name: "Account menu" }).click();
    await page.getByRole("menuitem", { name: "Log out" }).click();
    await expect(page).toHaveURL("/login");

    await page.getByLabel("Username").fill(username);
    await page.getByRole("button", { name: "Sign in with a passkey" }).click();
    await expect(page).toHaveURL("/");

    // The metadata line switches from "never used" to "last used {date}" once the credential has
    // signed someone in.
    await page.goto("/profile");
    await expect(
        page.getByRole("listitem").filter({ hasText: "last used" }),
    ).toHaveCount(1);
});

test("renames a passkey and the new name persists", async ({ page }) => {
    await registerUser(page.request);
    await addVirtualAuthenticator(page);
    await page.goto("/profile");
    await addPasskey(page, "Original name");
    await expect(page.getByText("Original name")).toBeVisible();

    await page.getByRole("button", { name: "Rename passkey" }).click();
    const renameDialog = page.getByRole("dialog", { name: "Rename passkey" });
    await expect(renameDialog.getByLabel("Name")).toHaveValue("Original name");
    await renameDialog.getByLabel("Name").fill("Renamed passkey");
    await renameDialog.getByRole("button", { name: "Rename" }).click();
    await expect(page.getByText("Renamed passkey")).toBeVisible();

    await page.reload();
    await expect(page.getByText("Renamed passkey")).toBeVisible();
});

test("deletes a passkey after confirming, and toasts", async ({ page }) => {
    await registerUser(page.request);
    await addVirtualAuthenticator(page);
    await page.goto("/profile");
    await addPasskey(page, "Doomed passkey");
    await expect(page.getByText("Doomed passkey")).toBeVisible();

    await page.getByRole("button", { name: "Delete passkey" }).click();
    const confirm = page.getByRole("alertdialog", {
        name: 'Delete passkey "Doomed passkey"?',
    });
    await expect(confirm).toBeVisible();
    await confirm.getByRole("button", { name: "Delete" }).click();
    await expectToast(page, "Passkey removed");
    await expect(page.getByText("Doomed passkey")).toHaveCount(0);
});

test("lists multiple passkeys with Added/never-used metadata", async ({
    page,
}) => {
    await registerUser(page.request);
    await page.goto("/profile");

    // Each passkey needs its own virtual authenticator: the server sends the user's existing
    // credential IDs as `excludeCredentials` on every registration ceremony (by design - it stops
    // the same physical authenticator re-registering for an account it's already enrolled in), so
    // re-using one authenticator for a second passkey would correctly be refused as a duplicate.
    // The second authenticator must use a non-"internal" transport - Chrome only supports one
    // internal (platform) authenticator per environment, but allows multiple non-internal ones,
    // and the backend doesn't force platform attachment on registration.
    await addVirtualAuthenticator(page);
    await addPasskey(page, "First passkey");
    await expect(page.getByText("First passkey")).toBeVisible();

    await addVirtualAuthenticator(page, "usb");
    await addPasskey(page, "Second passkey");
    await expect(page.getByText("Second passkey")).toBeVisible();

    const entries = page.getByRole("listitem").filter({ hasText: "Added " });
    await expect(entries).toHaveCount(2);
    await expect(entries.filter({ hasText: "never used" })).toHaveCount(2);
});

test("a non-cancellation failure toasts", async ({ page }) => {
    await registerUser(page.request);
    await addVirtualAuthenticator(page);

    // Register the route before navigating, not right before the click, so the mock is guaranteed
    // active before the registration ceremony can possibly reach the network - registering it
    // immediately before addPasskey() left a window where the real, unmocked verify endpoint could
    // win the race and register the passkey for real (Playwright's own guidance: set up routes
    // before the navigation/action that triggers the request).
    await page.route("**/api/passkeys/register/verify", (route) =>
        route.fulfill({
            status: 500,
            contentType: "application/json",
            body: JSON.stringify({ error: "Could not add passkey" }),
        }),
    );
    await page.goto("/profile");
    await addPasskey(page, "Will fail");
    await expectToast(page, "Could not add passkey");
});

test("a cancelled ceremony is silent - no error toast", async ({ page }) => {
    await registerUser(page.request);
    const { client, authenticatorId } = await addVirtualAuthenticator(page);
    // Flip the virtual authenticator to NOT verify the user. The RP's registration ceremony
    // resolves user verification as required in this case, so the authenticator refuses and the
    // browser rejects navigator.credentials.create() with a NotAllowedError - the same
    // DOMException name `isCancellation()` treats as a user-cancelled ceremony (web/src/lib/webauthn.ts).
    await client.send("WebAuthn.setUserVerified", {
        authenticatorId,
        isUserVerified: false,
    });
    await page.goto("/profile");
    await addPasskey(page, "Cancelled passkey");

    // Give the rejected ceremony time to settle, then assert no toast fired at all - neither an
    // error toast nor a success one.
    await page.waitForTimeout(1000);
    await expect(page.locator("[data-sonner-toast]")).toHaveCount(0);
    await expect(page.getByText("Cancelled passkey")).toHaveCount(0);
});

test("passkey sign-in with an empty username toasts instead of erroring", async ({
    page,
}) => {
    await page.goto("/login");
    await page.getByRole("button", { name: "Sign in with a passkey" }).click();
    await expectToast(page, "Enter your username, then use your passkey");
    await expect(page).toHaveURL("/login");
});
