import { afterEach, describe, expect, it, vi } from "vitest";
import { externalHref } from "./externalLink";

const ANDROID_CHROME =
    "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";
const IPHONE_SAFARI =
    "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1";
const DESKTOP_CHROME =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";
const ANDROID_FIREFOX =
    "Mozilla/5.0 (Android 14; Mobile; rv:127.0) Gecko/127.0 Firefox/127.0";
// An in-app browser. Carries Android + Chrome/<n>, but fails intent:// with
// ERR_UNKNOWN_URL_SCHEME and never reaches the fallback - note the `; wv)` token.
const ANDROID_WEBVIEW =
    "Mozilla/5.0 (Linux; Android 14; Pixel 8; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/126.0.0.0 Mobile Safari/537.36";
// The other Android Chromium browsers. All honour intent://; Samsung Internet is the default
// browser on Samsung phones, so this is not a rounding error.
const SAMSUNG_INTERNET =
    "Mozilla/5.0 (Linux; Android 14; SAMSUNG SM-S911B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/25.0 Chrome/121.0.0.0 Mobile Safari/537.36";
const EDGE_ANDROID =
    "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36 EdgA/126.0.0.0";
const OPERA_ANDROID =
    "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36 OPR/79.0.0.0";

function ua(value: string) {
    vi.stubGlobal("navigator", { userAgent: value });
}

afterEach(() => vi.unstubAllGlobals());

const REDDIT = "https://www.reddit.com/r/rust/comments/abc123/some_post/";
const YOUTUBE = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";

describe("externalHref on Android Chrome", () => {
    it("rewrites a reddit URL to an intent that targets the Reddit app", () => {
        ua(ANDROID_CHROME);
        const href = externalHref(REDDIT, "reddit");
        expect(href).toBe(
            "intent://www.reddit.com/r/rust/comments/abc123/some_post/" +
                "#Intent;scheme=https;package=com.reddit.frontpage;" +
                `S.browser_fallback_url=${encodeURIComponent(REDDIT)};end`,
        );
    });

    it("keeps the query string, which is where YouTube's video id lives", () => {
        ua(ANDROID_CHROME);
        const href = externalHref(YOUTUBE, "youtube");
        expect(href).toContain("intent://www.youtube.com/watch?v=dQw4w9WgXcQ#");
        expect(href).toContain("package=com.google.android.youtube");
    });

    it("carries the original URL as the fallback, so no app means a normal tab", () => {
        ua(ANDROID_CHROME);
        expect(externalHref(YOUTUBE, "youtube")).toContain(
            `S.browser_fallback_url=${encodeURIComponent(YOUTUBE)};end`,
        );
    });

    // Chrome finds the extras at the LAST `#` and splits them on `;`, so a fallback URL escaped
    // with anything weaker than encodeURIComponent (encodeURI leaves `;` `#` `&` raw) silently
    // corrupts the intent. Pin the property, not the expression.
    it("escapes the fallback so it survives Chrome's extras parse", () => {
        ua(ANDROID_CHROME);
        const gnarly =
            "https://www.youtube.com/watch?v=abc&t=90s&list=PL;x#t=90";
        const href = externalHref(gnarly, "youtube");

        expect(href.split("#")).toHaveLength(2);
        const fallback = href.match(
            /S\.browser_fallback_url=([^;]*);end$/,
        )?.[1];
        expect(fallback).toBeDefined();
        expect(decodeURIComponent(fallback as string)).toBe(gnarly);
        // The fragment cannot ride along in the intent's data section.
        expect(
            href.startsWith("intent://www.youtube.com/watch?v=abc&t=90s"),
        ).toBe(true);
    });

    it("leaves a URL whose host the app does not claim untouched", () => {
        ua(ANDROID_CHROME);
        // A reddit-kind item whose url somehow points off-site must not be handed to the app.
        expect(externalHref("https://evil.example/r/rust", "reddit")).toBe(
            "https://evil.example/r/rust",
        );
        expect(externalHref("https://youtu.be/abc", "youtube")).toContain(
            "intent://youtu.be/abc",
        );
    });

    it("leaves feed kinds with no app untouched", () => {
        ua(ANDROID_CHROME);
        expect(externalHref("https://example.com/post", "rss")).toBe(
            "https://example.com/post",
        );
        expect(externalHref("https://example.com/post", "atom")).toBe(
            "https://example.com/post",
        );
    });

    it("leaves non-https and malformed URLs untouched", () => {
        ua(ANDROID_CHROME);
        expect(externalHref("http://www.reddit.com/r/rust", "reddit")).toBe(
            "http://www.reddit.com/r/rust",
        );
        expect(externalHref("not a url", "reddit")).toBe("not a url");
    });
});

describe("externalHref on the other Android Chromium browsers", () => {
    it.each([
        ["Samsung Internet", SAMSUNG_INTERNET],
        ["Edge", EDGE_ANDROID],
        ["Opera", OPERA_ANDROID],
    ])("rewrites on %s, which is Chromium and honours intent://", (_, agent) => {
        ua(agent);
        expect(externalHref(REDDIT, "reddit")).toContain(
            "intent://www.reddit.com/",
        );
    });
});

describe("externalHref everywhere else", () => {
    it("leaves URLs untouched on iOS", () => {
        ua(IPHONE_SAFARI);
        expect(externalHref(REDDIT, "reddit")).toBe(REDDIT);
    });

    it("leaves URLs untouched on desktop Chrome", () => {
        ua(DESKTOP_CHROME);
        expect(externalHref(YOUTUBE, "youtube")).toBe(YOUTUBE);
    });

    it("leaves URLs untouched on Android Firefox, which does not handle intent://", () => {
        ua(ANDROID_FIREFOX);
        expect(externalHref(REDDIT, "reddit")).toBe(REDDIT);
    });

    it("leaves URLs untouched in an Android WebView, which would show an error page", () => {
        ua(ANDROID_WEBVIEW);
        expect(externalHref(REDDIT, "reddit")).toBe(REDDIT);
    });
});
