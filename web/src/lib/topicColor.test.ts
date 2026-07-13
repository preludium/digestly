import { describe, expect, it } from "vitest";
import { topicBadgeClass } from "./topicColor";

describe("topicBadgeClass", () => {
    it("is deterministic for the same name", () => {
        expect(topicBadgeClass("Tech")).toBe(topicBadgeClass("Tech"));
    });
    it("returns a bg + text class pair", () => {
        expect(topicBadgeClass("Science")).toMatch(
            /^bg-badge-[1-8] text-badge-[1-8]-foreground$/,
        );
    });
    it("spreads names across the palette", () => {
        const names = [
            "Tech",
            "Science",
            "News",
            "Gaming",
            "Design",
            "Music",
            "Business",
            "Other",
        ];
        const distinct = new Set(names.map(topicBadgeClass));
        expect(distinct.size).toBeGreaterThan(3);
    });
});
