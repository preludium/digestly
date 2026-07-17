import { defineConfig, devices } from "@playwright/test";
import { ADMIN, APP_URL } from "./e2e/support/api";

export default defineConfig({
    testDir: "./e2e",
    testIgnore: "**/screenshots/**",
    fullyParallel: false,
    workers: 1,
    reporter: process.env.CI
        ? [["html", { open: "never" }], ["github"]]
        : [["list"]],
    use: {
        baseURL: APP_URL,
        trace: "retain-on-failure",
        screenshot: "only-on-failure",
    },
    projects: [
        {
            name: "chromium",
            use: { ...devices["Desktop Chrome"] },
        },
    ],
    timeout: 60_000,
    expect: {
        timeout: 10_000,
    },
    webServer: [
        {
            command: "node e2e/support/fixture-server.mjs",
            url: "http://localhost:8098/health",
            reuseExistingServer: false,
            timeout: 30_000,
            env: { FIXTURE_PORT: "8098" },
        },
        {
            // No globalSetup/globalTeardown (stage-1 decision): folding the DB wipe into the
            // launch command avoids relying on unverified ordering between Playwright's
            // globalSetup and its webServer startup, which isn't documented as sequential.
            command: "rm -rf .e2e-data && cargo run --release",
            cwd: "../backend",
            url: "http://localhost:8099/api/auth/registration",
            reuseExistingServer: false,
            timeout: 300_000,
            env: {
                // Bind to 127.0.0.1 (not localhost) so the OS resolves a single, unambiguous
                // loopback address for the listener, while the browser and RP_ORIGIN below still
                // use "localhost" - WebAuthn's rp.id must be the hostname the page is served from,
                // and "localhost" is what browsers special-case as a secure context without TLS.
                BIND_ADDR: "127.0.0.1:8099",
                DATA_DIR: ".e2e-data",
                STATIC_DIR: "../web/dist",
                RP_ID: "localhost",
                RP_ORIGIN: "http://localhost:8099",
                SECRET_KEY: "e2e-secret-key-0123456789",
                ADMIN_PASSWORD: ADMIN.password,
            },
        },
    ],
});
