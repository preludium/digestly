import { useState } from "react";
import { HeartPulse } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { EmptyState } from "@/components/common/EmptyState";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { FeedEditModal } from "@/components/feeds/FeedEditModal";
import { useFeedHealth, useFeeds, useRefreshFeed, useUnsubscribe } from "@/hooks/useFeeds";
import { formatDateTime, kindIcon } from "@/lib/format";
import type { Feed, FeedHealth, FeedStatus } from "@/lib/types";
import { toast } from "@/stores/toast";

const STATUS_BADGE: Record<FeedStatus, { label: string; variant: "outline" | "secondary" | "destructive" }> = {
  ok: { label: "OK", variant: "outline" },
  failing: { label: "failing", variant: "secondary" },
  disabled: { label: "disabled", variant: "destructive" },
};

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
        <h1 className="text-2xl font-bold">Feed health</h1>
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            className="size-4 accent-primary"
            checked={problemsOnly}
            onChange={(e) => setProblemsOnly(e.target.checked)}
          />
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
                  <TableHead>Next retry</TableHead>
                  <TableHead className="text-right">Actions</TableHead>
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
  const badge = STATUS_BADGE[row.status];

  return (
    <TableRow>
      <TableCell>
        <div className="flex items-center gap-2">
          <span>{kindIcon(row.kind)}</span>
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
      <TableCell className="text-muted-foreground">{formatDateTime(row.next_fetch_at)}</TableCell>
      <TableCell>
        <div className="flex justify-end gap-2">
          <Button
            size="sm"
            variant="outline"
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
            onClick={() => {
              if (confirm(`Unsubscribe from "${row.title}"?`)) {
                unsubscribe.mutate(row.id, { onSuccess: () => toast("Unsubscribed", "success") });
              }
            }}
          >
            Remove
          </Button>
        </div>
      </TableCell>
    </TableRow>
  );
}
