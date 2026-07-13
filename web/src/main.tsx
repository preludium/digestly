import { QueryClientProvider } from "@tanstack/react-query";
import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import { App } from "./App";
import { Toaster } from "./components/common/Toaster";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { registerServiceWorker } from "./lib/pwa";
import { queryClient } from "./lib/queryClient";
import { applyDensity, applyTheme, useUiStore } from "./stores/ui";
import "@fontsource-variable/fraunces";
import "@fontsource-variable/instrument-sans";
import "./index.css";

// Apply the persisted theme + density before first paint.
applyTheme(useUiStore.getState().theme);
applyDensity(useUiStore.getState().density);

// Register the PWA service worker (app-shell + offline reading). No-op in dev.
registerServiceWorker();

ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
        <ErrorBoundary>
            <QueryClientProvider client={queryClient}>
                <BrowserRouter>
                    <App />
                </BrowserRouter>
                <Toaster />
            </QueryClientProvider>
        </ErrorBoundary>
    </React.StrictMode>,
);
