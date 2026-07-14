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
 * Android Chrome is the only browser where handing off to a native app is worth attempting.
 *
 * App Links (Android) and Universal Links (iOS) deliberately do NOT fire for JS-initiated or
 * target=_blank navigations, so a plain https link can never open the app. `intent://` is the one
 * mechanism with a built-in installed-check and fallback, and it is Chrome-specific.
 *
 * UA sniffing is unavoidable here and inherently brittle (e.g. "request desktop site" hides
 * `Android`). The failure mode is benign: we return the plain https URL and the link behaves
 * exactly as it does today.
 *
 * The `wv` exclusion is the one that isn't cosmetic. An Android WebView (in-app browsers) carries
 * both `Android` and `Chrome/\d`, but fails an `intent://` navigation with ERR_UNKNOWN_URL_SCHEME
 * and never reaches the fallback - the one context where guessing wrong shows the user an error
 * page instead of the article.
 */
function isAndroidChrome(): boolean {
    if (typeof navigator === "undefined") return false;
    const ua = navigator.userAgent;
    return (
        /Android/.test(ua) &&
        /Chrome\/\d/.test(ua) &&
        !/\bwv\b|Firefox|EdgA|OPR|SamsungBrowser/.test(ua)
    );
}

/**
 * The href to use for an item's original link.
 *
 * On Android Chrome, reddit and youtube URLs become `intent://` URLs: Chrome opens the native app
 * when it is installed, and otherwise navigates to `S.browser_fallback_url` - the same https URL
 * we started with. Everywhere else (iOS, desktop, other feed kinds) the URL is returned untouched.
 *
 * iOS is deliberately left alone: custom schemes (`vnd.youtube:`, `reddit://`) raise a blocking
 * "Cannot open page" alert when the app is missing, which is worse than the browser tab we get now.
 */
export function externalHref(url: string, kind: FeedKind): string {
    const app = APPS[kind];
    if (!app || !isAndroidChrome()) return url;

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
