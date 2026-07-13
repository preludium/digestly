import { describe, expect, it } from "vitest";
import type { Category } from "@/lib/types";
import { sortCategoriesOtherLast } from "./manage.helpers";

const cat = (id: number, name: string): Category =>
    ({
        id,
        name,
        feed_count: 0,
        deletable: name !== "Other",
        position: id,
    }) as Category;

describe("sortCategoriesOtherLast", () => {
    it("puts Other last regardless of input order", () => {
        const sorted = sortCategoriesOtherLast([
            cat(1, "Other"),
            cat(2, "Tech"),
            cat(3, "News"),
        ]);
        expect(sorted.map((c) => c.name)).toEqual(["Tech", "News", "Other"]);
    });
    it("keeps relative order of the rest", () => {
        const sorted = sortCategoriesOtherLast([
            cat(1, "B"),
            cat(2, "Other"),
            cat(3, "A"),
        ]);
        expect(sorted.map((c) => c.name)).toEqual(["B", "A", "Other"]);
    });
});
