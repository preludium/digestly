/**
 * Uses react-dom/server renderToStaticMarkup (Node, no jsdom/RTL needed) to
 * exercise the Reddit logo fallback path. The project has no RTL/jsdom harness;
 * this is the minimal approach that tests real React rendering without adding
 * new test infrastructure.
 */
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { Item } from "@/lib/types";
import { ItemCard } from "./ItemCard";

const redditItemNoImage: Item = {
    id: 1,
    feed_id: 10,
    category: "Tech",
    feed_title: "r/rust",
    kind: "reddit",
    content_type: "reading",
    title: "Some Reddit post",
    url: "https://www.reddit.com/r/rust/comments/abc",
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
};

describe("ItemCard – Reddit logo fallback", () => {
    it("renders a Reddit item with no image_url without throwing and includes the SVG logo", () => {
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
