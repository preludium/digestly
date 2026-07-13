import type { Category } from "@/lib/types";

export function sortCategoriesOtherLast(categories: Category[]): Category[] {
    return [...categories].sort(
        (a, b) => Number(a.name === "Other") - Number(b.name === "Other"),
    );
}
