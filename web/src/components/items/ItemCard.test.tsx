/**
 * The project has no RTL/jsdom harness. The Reddit-logo path is exercised with
 * react-dom/server renderToStaticMarkup (Node, no jsdom/RTL needed); the favicon
 * fallback's branching logic is covered via the exported `faviconOf` helper.
 * This is the minimal approach that tests real behavior without adding new
 * test infrastructure.
 */
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { Item } from "@/lib/types";
import { faviconOf, ItemCard } from "./ItemCard";

function makeItem(overrides: Partial<Item> = {}): Item {
    return {
        id: 1,
        feed_id: 1,
        category: "Tech",
        feed_title: "Some Blog",
        kind: "rss",
        content_type: "reading",
        title: "A post",
        url: "https://example.com/post",
        author: null,
        snippet: null,
        image_url: null,
        published_at: null,
        is_read: false,
        is_starred: false,
        reading_time_secs: null,
        duration_secs: null,
        score: null,
        comments_count: null,
        upvote_ratio: null,
        transcript_status: "none",
        has_summary: false,
        site_url: null,
        feed_icon_url: null,
        ...overrides,
    };
}

describe("ItemCard – Reddit logo fallback", () => {
    it("renders a Reddit item with no image_url without throwing and includes the SVG logo", () => {
        const redditItemNoImage = makeItem({
            id: 1,
            feed_id: 10,
            feed_title: "r/rust",
            kind: "reddit",
            title: "Some Reddit post",
            url: "https://www.reddit.com/r/rust/comments/abc",
        });
        const html = renderToStaticMarkup(
            <ItemCard item={redditItemNoImage} onOpen={vi.fn()} />,
        );
        // The inline RedditLogo component must produce actual SVG markup,
        // not a raw string/URL being used as a tag name (which would throw or
        // produce invalid output).
        expect(html).toContain("<svg");
        // The Reddit brand colour is a reliable marker that the correct SVG
        // content was rendered (not the generic ImageIcon fallback).
        expect(html).toContain("#FF4500");
    });
});

describe("faviconOf", () => {
    it("derives the origin favicon from a blog's https site_url", () => {
        const item = makeItem({ site_url: "https://blog.example.com/about" });
        expect(faviconOf(item)).toBe("https://blog.example.com/favicon.ico");
    });

    it("returns null when site_url is missing, even with an https item url", () => {
        const item = makeItem({
            site_url: null,
            url: "https://third-party.example.com/posts/1",
        });
        expect(faviconOf(item)).toBeNull();
    });

    it("returns null for reddit items", () => {
        const item = makeItem({
            kind: "reddit",
            site_url: "https://www.reddit.com",
        });
        expect(faviconOf(item)).toBeNull();
    });

    it("returns null for youtube items", () => {
        const item = makeItem({
            kind: "youtube",
            site_url: "https://www.youtube.com",
        });
        expect(faviconOf(item)).toBeNull();
    });

    it("returns null for a non-https site_url", () => {
        const item = makeItem({ site_url: "http://blog.example.com" });
        expect(faviconOf(item)).toBeNull();
    });

    it("returns null when there is no usable url at all", () => {
        const item = makeItem({ site_url: null, url: null });
        expect(faviconOf(item)).toBeNull();
    });

    it("returns null for a malformed url", () => {
        const item = makeItem({ site_url: "not a url" });
        expect(faviconOf(item)).toBeNull();
    });
});
