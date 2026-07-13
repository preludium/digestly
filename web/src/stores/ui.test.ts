import { beforeEach, describe, expect, it } from "vitest";
import { useUiStore } from "./ui";

describe("add-feed modal category preselection", () => {
    beforeEach(() => {
        useUiStore.setState({ addFeedOpen: false, addFeedCategoryId: null });
    });

    it("carries the category id when opened from a category tile", () => {
        useUiStore.getState().setAddFeedOpen(true, 7);
        expect(useUiStore.getState().addFeedOpen).toBe(true);
        expect(useUiStore.getState().addFeedCategoryId).toBe(7);
    });

    it("has no preselection when opened without a category (top bar)", () => {
        useUiStore.getState().setAddFeedOpen(true, 7);
        useUiStore.getState().setAddFeedOpen(false);
        useUiStore.getState().setAddFeedOpen(true);
        expect(useUiStore.getState().addFeedCategoryId).toBeNull();
    });

    it("clears the preselection on close", () => {
        useUiStore.getState().setAddFeedOpen(true, 7);
        useUiStore.getState().setAddFeedOpen(false);
        expect(useUiStore.getState().addFeedCategoryId).toBeNull();
    });
});
