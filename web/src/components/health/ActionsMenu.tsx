import { MoreVertical, Pencil, RefreshCw, Trash2 } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { Button } from "@/components/ui/button";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useRefreshFeed, useUnsubscribe } from "@/hooks/useFeeds";
import type { FeedHealth } from "@/lib/types";

export function ActionsMenu({
    row,
    onEdit,
}: {
    row: FeedHealth;
    onEdit: (id: number) => void;
}) {
    const refresh = useRefreshFeed();
    const unsubscribe = useUnsubscribe();
    const [removing, setRemoving] = useState<FeedHealth | null>(null);

    return (
        <>
            <DropdownMenu>
                <DropdownMenuTrigger asChild>
                    <Button variant="ghost" size="icon" aria-label="Actions">
                        <MoreVertical className="size-4" />
                    </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                    <DropdownMenuItem
                        disabled={refresh.isPending}
                        onClick={() =>
                            refresh.mutate(row.id, {
                                onSuccess: () =>
                                    toast.success(
                                        row.status === "disabled"
                                            ? "Re-enabled"
                                            : "Retrying…",
                                    ),
                            })
                        }
                    >
                        <RefreshCw className="size-4" />{" "}
                        {row.status === "disabled" ? "Re-enable" : "Retry"}
                    </DropdownMenuItem>
                    <DropdownMenuItem onClick={() => onEdit(row.id)}>
                        <Pencil className="size-4" /> Edit
                    </DropdownMenuItem>
                    <DropdownMenuItem
                        className="text-destructive focus:bg-destructive/10 focus:text-destructive"
                        onClick={() => setRemoving(row)}
                    >
                        <Trash2 className="size-4" /> Remove
                    </DropdownMenuItem>
                </DropdownMenuContent>
            </DropdownMenu>
            <ConfirmDialog
                open={!!removing}
                onOpenChange={(v) => !v && setRemoving(null)}
                title={`Unsubscribe from "${row.title}"?`}
                confirmLabel="Unsubscribe"
                destructive
                onConfirm={() => {
                    // biome-ignore lint/style/noNonNullAssertion: existing baseline
                    unsubscribe.mutate(removing!.id, {
                        onSuccess: () => {
                            toast.success("Unsubscribed");
                            setRemoving(null);
                        },
                    });
                }}
            />
        </>
    );
}
