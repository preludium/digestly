import { expect, test } from "@playwright/test";
import { registerUser } from "./support/api";

// CDP's WebAuthn domain (virtual authenticator) is only available on Chromium.
test.skip(
    ({ browserName }) => browserName !== "chromium",
    "CDP/WebAuthn is Chromium-only",
);

test("adds a passkey and signs in with it", async ({ page }) => {
    const { username } = await registerUser(page.request);
    await page.goto("/");
    await expect(page).toHaveURL("/");

    const client = await page.context().newCDPSession(page);
    await client.send("WebAuthn.enable", { enableUI: false });
    const { authenticatorId } = await client.send(
        "WebAuthn.addVirtualAuthenticator",
        {
            options: {
                protocol: "ctap2",
                transport: "internal",
                hasResidentKey: true,
                hasUserVerification: true,
                isUserVerified: true,
                automaticPresenceSimulation: true,
            },
        },
    );

    await page.goto("/profile");
    await page.getByRole("button", { name: "Add a passkey" }).click();
    const dialog = page.getByRole("dialog", { name: "Add a passkey" });
    await dialog.getByLabel("Passkey name").fill("E2E passkey");
    await dialog.getByRole("button", { name: "Continue" }).click();

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
});
