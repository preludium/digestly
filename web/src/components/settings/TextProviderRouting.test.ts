import { describe, expect, it } from "vitest";
import type { AiProvider } from "@/lib/types";
import { effectiveTextProviderIds } from "./TextProviderRouting";

describe("effectiveTextProviderIds", () => {
    it("preserves route order while excluding the dedicated video provider", () => {
        const providers = [
            { id: 1, is_video_only: false },
            { id: 2, is_video_only: true },
            { id: 3, is_video_only: false },
        ] as AiProvider[];

        expect(effectiveTextProviderIds(providers, [2, 3, 1])).toEqual([3, 1]);
    });
});
