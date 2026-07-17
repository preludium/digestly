import { kindIcon } from "@/lib/format";
import type { FeedHealth } from "@/lib/types";
import { YoutubeIcon } from "./YoutubeIcon";

export function FeedIcon({ row }: { row: FeedHealth }) {
    return row.kind === "youtube" ? (
        <YoutubeIcon className="size-5 shrink-0 text-destructive" />
    ) : (
        <span className="text-lg">{kindIcon(row.kind)}</span>
    );
}
