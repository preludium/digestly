import { Link } from "react-router-dom";
import { Newspaper, Bell, BellOff, AlertTriangle } from "lucide-react";
import { Card } from "@/components/ui/card";
import { Spinner } from "@/components/ui/spinner";
import { EmptyState } from "@/components/common/EmptyState";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { useDigests } from "@/hooks/useDigest";
import { formatDateTime } from "@/lib/format";

/** Digests list (prompt.md §9.8): the current user's archived digests, newest first. */
export function Digests() {
  const digests = useDigests();

  return (
    <div className="space-y-4">
      <h1 className="font-display text-2xl font-semibold tracking-tight">Digests</h1>

      {digests.isLoading ? (
        <div className="flex justify-center py-10"><Spinner className="size-6" /></div>
      ) : digests.isError ? (
        <ErrorBanner error={digests.error} />
      ) : digests.data && digests.data.length > 0 ? (
        <ul className="space-y-2">
          {digests.data.map((d) => (
            <li key={d.id}>
              <Link to={`/digests/${d.id}`}>
                <Card className="flex items-center justify-between gap-3 p-4 transition-colors hover:bg-muted">
                  <div className="min-w-0">
                    <p className="font-medium">Digest — {formatDateTime(d.created_at)}</p>
                    <p className="text-xs text-muted-foreground">
                      {d.item_count} item{d.item_count === 1 ? "" : "s"} · {formatDateTime(d.period_start)} → {formatDateTime(d.period_end)}
                    </p>
                  </div>
                  <div className="flex shrink-0 items-center gap-2 text-muted-foreground">
                    {d.error && <AlertTriangle className="size-4 text-destructive" aria-label="Delivery error" />}
                    {d.notified ? (
                      <Bell className="size-4 text-primary" aria-label="Pushed to ntfy" />
                    ) : (
                      <BellOff className="size-4" aria-label="Not pushed" />
                    )}
                  </div>
                </Card>
              </Link>
            </li>
          ))}
        </ul>
      ) : (
        <EmptyState
          icon={<Newspaper className="size-8" />}
          title="No digests yet"
          description="Digests appear here on the schedule set by your admin, or after a manual run."
        />
      )}
    </div>
  );
}
