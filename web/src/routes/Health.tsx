import { useState } from "react";
import { HeartPulse } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ConfirmDialog } from "@/components/common/ConfirmDialog";
import { EmptyState } from "@/components/common/EmptyState";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { FeedEditModal } from "@/components/feeds/FeedEditModal";
import { useFeedHealth, useFeeds, useRefreshFeed, useUnsubscribe } from "@/hooks/useFeeds";
import { formatDateTime, kindIcon } from "@/lib/format";
import type { Feed, FeedHealth, FeedStatus } from "@/lib/types";
import { toast } from "@/stores/toast";

const STATUS_BADGE: Record<FeedStatus, { label: string; variant: "success" | "warning" | "destructive" }> = {
  ok: { label: "OK", variant: "success" },
  failing: { label: "failing", variant: "warning" },
  disabled: { label: "disabled", variant: "destructive" },
};

function YoutubeIcon({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="currentColor" aria-label="YouTube">
      <path d="M19.615 3.184c-3.604-.246-11.631-.245-15.23 0-3.897.266-4.356 2.62-4.385 8.816.029 6.185.484 8.549 4.385 8.816 3.6.245 11.626.246 15.23 0 3.897-.266 4.356-2.62 4.385-8.816-.029-6.185-.484-8.549-4.385-8.816zm-10.615 12.816v-8l8 3.993-8 4.007z" />
    </svg>
  );
}

/** Feed health / diagnostics (prompt.md §9.6). Failing/disabled feeds surfaced, never dropped. */
export function Health() {
  const health = useFeedHealth();
  const feeds = useFeeds();
  const [problemsOnly, setProblemsOnly] = useState(false);
  const [editing, setEditing] = useState<Feed | null>(null);

  const rows = (health.data ?? []).filter((h) => !problemsOnly || h.status !== "ok");
  const noFeeds = (health.data?.length ?? 0) === 0;

  const openEdit = (id: number) => {
    const feed = feeds.data?.find((f) => f.id === id);
    if (feed) setEditing(feed);
  };

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="font-display text-2xl font-semibold tracking-tight">Feed health</h1>
        <label htmlFor="problems-only" className="flex items-center gap-2 text-sm">
          <Switch id="problems-only" checked={problemsOnly} onCheckedChange={setProblemsOnly} />
          Problems only
        </label>
      </div>

      {health.isError && <ErrorBanner error={health.error} />}

      {health.isLoading && (
        <div className="space-y-2">
          {[0, 1, 2].map((i) => (
            <Skeleton key={i} className="h-12 w-full" />
          ))}
        </div>
      )}

      {!health.isLoading && !health.isError && (noFeeds || rows.length === 0) && (
        <EmptyState
          icon={<HeartPulse className="size-8" />}
          title={noFeeds ? "No feeds yet" : "All feeds healthy"}
          description={noFeeds ? "Add a feed to see its health here." : "Nothing to worry about right now 🎉"}
        />
      )}

      {!health.isLoading && !health.isError && rows.length > 0 && (
        <Card>
          <CardContent className="overflow-x-auto p-0 sm:p-2">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Feed</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Last fetch</TableHead>
                  <TableHead>Fails</TableHead>
                  <TableHead className="text-right"><span className="sr-only">Actions</span></TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {rows.map((h) => (
                  <HealthRow key={h.id} row={h} onEdit={openEdit} />
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}

      <FeedEditModal feed={editing} onClose={() => setEditing(null)} />
    </div>
  );
}

function HealthRow({ row, onEdit }: { row: FeedHealth; onEdit: (id: number) => void }) {
  const refresh = useRefreshFeed();
  const unsubscribe = useUnsubscribe();
  const [removing, setRemoving] = useState<FeedHealth | null>(null);
  const badge = STATUS_BADGE[row.status];

  return (
    <TableRow>
      <TableCell>
        <div className="flex items-center gap-2">
          <span>{row.kind === "youtube" ? <YoutubeIcon className="size-5 shrink-0 text-destructive" /> : <span className="text-lg">{kindIcon(row.kind)}</span>}</span>
          <div className="min-w-0">
            <p className="truncate font-medium">{row.title}</p>
            {row.last_error && <p className="truncate text-xs text-destructive">{row.last_error}</p>}
          </div>
        </div>
      </TableCell>
      <TableCell>
        <Badge variant={badge.variant}>{badge.label}</Badge>
      </TableCell>
      <TableCell className="text-muted-foreground">{formatDateTime(row.last_fetch_at)}</TableCell>
      <TableCell className="text-muted-foreground">{row.failure_count}</TableCell>
      <TableCell>
        <div className="flex justify-end gap-2">
          <Button
            size="sm"
            variant="outline"
            className="text-success border-success/40 hover:bg-success/10"
            disabled={refresh.isPending}
            onClick={() =>
              refresh.mutate(row.id, {
                onSuccess: () => toast(row.status === "disabled" ? "Re-enabled" : "Retrying…", "success"),
              })
            }
          >
            {row.status === "disabled" ? "Re-enable" : "Retry now"}
          </Button>
          <Button size="sm" variant="ghost" onClick={() => onEdit(row.id)}>
            Edit
          </Button>
          <Button
            size="sm"
            variant="ghost"
            className="text-destructive hover:bg-destructive/10"
            onClick={() => setRemoving(row)}
          >
            Remove
          </Button>
          <ConfirmDialog
            open={!!removing}
            onOpenChange={(v) => !v && setRemoving(null)}
            title={`Unsubscribe from "${row.title}"?`}
            confirmLabel="Unsubscribe"
            destructive
            onConfirm={() => {
              unsubscribe.mutate(removing!.id, { onSuccess: () => { toast("Unsubscribed", "success"); setRemoving(null); } });
            }}
          />
        </div>
      </TableCell>
    </TableRow>
  );
}
