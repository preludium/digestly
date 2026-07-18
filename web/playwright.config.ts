import { defineConfig, devices } from "@playwright/test";
import { ADMIN, APP_URL } from "./e2e/support/api";

// Specs are split into two projects by shared state (issue #43):
//
// - Most specs are user-scoped and order-independent (support/api.ts mints a unique username per
//   call), so they run in the "parallel" project with fullyParallel:true.
// - A few specs mutate instance-wide singletons - the "Open registration" admin toggle
//   (admin.spec.ts) and the active AI provider routing config (ai-routing.spec.ts) - where a
//   second worker flipping the same global row mid-test is a real race, not a theoretical one:
//   `allow_registration` gates every `registerUser()` call in the whole suite, and the AI
//   provider/settings rows are read by any concurrent summarize call. Those are tagged `@serial`
//   and confined to a single-worker project.
//
// The "parallel" project declares a `dependencies` on "serial" so the singleton-touching tests
// always run to completion before any parallel-project test starts - tagging alone only prevents
// the @serial tests from racing each other; it does not stop a *different* project's worker from
// registering a user while `allow_registration` is flipped off. Ordering the projects removes
// that gap without giving up worker parallelism for the bulk of the suite.
const SERIAL_TAG = /@serial/;

export default defineConfig({
    testDir: "./e2e",
    testIgnore: "**/screenshots/**",
    workers: 2,
    reporter: process.env.CI
        ? [["html", { open: "never" }], ["github"]]
        : [["list"]],
    use: {
        baseURL: APP_URL,
        trace: "retain-on-failure",
        video: "retain-on-failure",
        screenshot: "only-on-failure",
    },
    projects: [
        {
            name: "serial",
            fullyParallel: false,
            workers: 1,
            grep: SERIAL_TAG,
            use: { ...devices["Desktop Chrome"] },
        },
        {
            name: "parallel",
            fullyParallel: true,
            dependencies: ["serial"],
            grepInvert: SERIAL_TAG,
            use: { ...devices["Desktop Chrome"] },
        },
    ],
    timeout: 60_000,
    expect: {
        timeout: 10_000,
    },
    webServer: [
        {
            // stdout/stderr aren't retained by Playwright by default (see Playwright's webServer
            // docs: "To see the stdout, you can set DEBUG=pw:webserver"), and CI has no terminal to
            // read that from anyway. `tee` duplicates the fixture server's log to a file CI can
            // upload while still streaming to the console for local runs.
            command:
                "mkdir -p e2e-logs && node e2e/support/fixture-server.mjs 2>&1 | tee e2e-logs/fixture-server.log",
            url: "http://localhost:8098/health",
            reuseExistingServer: false,
            timeout: 30_000,
            env: { FIXTURE_PORT: "8098" },
        },
        {
            // No globalSetup/globalTeardown (stage-1 decision): folding the DB wipe into the
            // launch command avoids relying on unverified ordering between Playwright's
            // globalSetup and its webServer startup, which isn't documented as sequential.
            //
            // The backend's stdout/stderr is the highest-value diagnostic artifact for a CI-only
            // failure (every ingest/digest/AI/ntfy flow is backend behavior) - `tee` captures it to
            // a file without losing the local terminal output.
            command:
                "rm -rf .e2e-data && mkdir -p ../web/e2e-logs && ./target/release/digestly 2>&1 | tee ../web/e2e-logs/backend.log",
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
