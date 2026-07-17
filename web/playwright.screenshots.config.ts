import { defineConfig } from "@playwright/test";
import baseConfig from "./playwright.config";

export default defineConfig(baseConfig, {
    testIgnore: [],
    outputDir: "test-results/screenshots",
});
