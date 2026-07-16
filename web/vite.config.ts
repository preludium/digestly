import { fileURLToPath, URL } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// Built to static assets served by the Rust binary (single deployable service).
// In dev, /api is proxied to the backend so requests work without CORS.
export default defineConfig({
    plugins: [react(), tailwindcss()],
    resolve: {
        alias: {
            "@": fileURLToPath(new URL("./src", import.meta.url)),
        },
    },
    build: {
        outDir: "dist",
        emptyOutDir: true,
    },
    server: {
        proxy: {
            "/api": {
                target: "http://localhost:8080",
                changeOrigin: true,
            },
        },
    },
    test: {
        // Playwright specs live under e2e/*.spec.ts and have their own runner; without this,
        // vitest's default include (**/*.{test,spec}.*) would also pick them up.
        include: ["src/**/*.test.{ts,tsx}"],
    },
});
