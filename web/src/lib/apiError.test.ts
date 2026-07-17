import { describe, expect, it } from "vitest";
import { ApiError } from "./api";
import { apiError } from "./apiError";

describe("apiError", () => {
    it("returns the message of a plain Error", () => {
        expect(apiError(new Error("boom"), "fallback")).toBe("boom");
    });
    it("returns the message of an ApiError (subclass of Error)", () => {
        expect(apiError(new ApiError(500, "nope"), "fallback")).toBe("nope");
    });
    it("returns the fallback for a non-Error", () => {
        expect(apiError("string thrown", "fallback")).toBe("fallback");
        expect(apiError(undefined, "fallback")).toBe("fallback");
    });
});
