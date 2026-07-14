import type { FeedKind } from "@/lib/types";

/** Android package names for the apps we can hand off to, and the hosts each one actually claims. */
const APPS: Partial<Record<FeedKind, { pkg: string; hosts: RegExp }>> = {
    reddit: {
        pkg: "com.reddit.frontpage",
        hosts: /(^|\.)reddit\.com$/,
    },
    youtube: {
        pkg: "com.google.android.youtube",
        hosts: /(^|\.)(youtube\.com|youtu\.be)$/,
    },
};

/**
 * Whether this browser is known to handle `intent://` correctly.
 *
 * App Links (Android) and Universal Links (iOS) deliberately do NOT fire for JS-initiated or
 * target=_blank navigations, so a plain https link can never open the app. `intent://` is the one
 * mechanism with a built-in installed-check and fallback.
 *
 * This is an ALLOWLIST on purpose, not "is it Chromium". Being Chromium does not imply handling
 * intent:// - a browser that mishandles it strands the user on a blank tab or an error page with
 * the article nowhere in sight, which is strictly worse than the plain link they get today. So a
 * browser is opted in only with evidence, and absence of evidence means opted out (benign: the
 * link behaves exactly as it does now).
 *
 * - Chrome: documented and supported. https://developer.chrome.com/docs/android/intents
 * - Android WebView (in-app browsers, `wv` token): fails with ERR_UNKNOWN_URL_SCHEME and never
 *   reaches the fallback. Excluded.
 * - Samsung Internet: open, unresolved bug report - intent links "don't work", reported to open an
 *   empty tab. https://github.com/SamsungInternet/support/issues/71 Excluded.
 * - Edge / Opera on Android: no evidence either way found. Excluded until someone checks on a
 *   device.
 * - Firefox: Gecko, carries no `Chrome/<n>` token, so it never reaches the exclusions anyway.
 *
 * UA sniffing is unavoidable here and inherently brittle (e.g. "request desktop site" hides
 * `Android`). Every failure mode is a plain https link.
 */
function handlesIntentUrls(): boolean {
    if (typeof navigator === "undefined") return false;
    const ua = navigator.userAgent;
    return (
        /Android/.test(ua) &&
        /Chrome\/\d/.test(ua) &&
        !/\bwv\b|SamsungBrowser|EdgA|OPR/.test(ua)
    );
}

/**
 * The href to use for an item's original link.
 *
 * On Android Chrome, reddit and youtube URLs become `intent://` URLs: Chrome opens the native app
 * when it is installed, and otherwise navigates to `S.browser_fallback_url` - the same https URL
 * we started with. Everywhere else (iOS, desktop, browsers not known to handle intent://, other
 * feed kinds) the URL is returned untouched.
 *
 * iOS is deliberately left alone: custom schemes (`vnd.youtube:`, `reddit://`) raise a blocking
 * "Cannot open page" alert when the app is missing, which is worse than the browser tab we get now.
 */
export function externalHref(url: string, kind: FeedKind): string {
    const app = APPS[kind];
    if (!app || !handlesIntentUrls()) return url;

    let u: URL;
    try {
        u = new URL(url);
    } catch {
        return url;
    }
    // Only https survives the round-trip through `scheme=https`.
    if (u.protocol !== "https:") return url;
    // Ingest only ever stores on-site permalinks for these kinds, but the host check keeps that
    // invariant local instead of relying on the Rust side to hold it forever.
    if (!app.hosts.test(u.host)) return url;

    // The fragment is deliberately dropped from the intent's data: the data URI ends where
    // `#Intent;` begins, so it cannot carry one. The fallback URL keeps it. encodeURIComponent is
    // load-bearing - it escapes the `;` `#` `&` that would otherwise break Chrome's extras parse.
    const fallback = encodeURIComponent(url);
    return `intent://${u.host}${u.pathname}${u.search}#Intent;scheme=https;package=${app.pkg};S.browser_fallback_url=${fallback};end`;
}
