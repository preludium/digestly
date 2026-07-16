import { expect, test } from "@playwright/test";
import { registerUser } from "./support/api";

test("General settings: timezone autosaves and persists after reload", async ({
    page,
}) => {
    // Register a fresh user; page.request shares the browser context's cookie jar,
    // so page.goto() will be authenticated without storageState.
    await registerUser(page.request);

    await page.goto("/settings");
    await page.getByRole("tab", { name: "General" }).click();

    // Arm the listener before triggering the fill so the debounced autosave response
    // cannot be missed even if the debounce interval is very short.
    const saveResponse = page.waitForResponse(
        (response) =>
            response.url().includes("/api/settings") &&
            response.request().method() === "PUT" &&
            response.ok(),
    );
    await page.getByPlaceholder("Europe/Warsaw").fill("America/New_York");
    await saveResponse;

    // Reload and re-open the General tab to prove the value round-tripped through
    // PUT /api/settings and back. A UserSettings serde field rename that is not
    // mirrored in types.ts causes the frontend to read undefined here and the input
    // comes back empty - this is the drift-catcher assertion.
    await page.reload();
    await page.getByRole("tab", { name: "General" }).click();

    await expect(page.getByPlaceholder("Europe/Warsaw")).toHaveValue(
        "America/New_York",
    );
});
