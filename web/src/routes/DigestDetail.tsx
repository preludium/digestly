import { Link, useParams } from "react-router-dom";
import { ArrowLeft, AlertTriangle, Sparkles } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Spinner } from "@/components/ui/spinner";
import { EmptyState } from "@/components/common/EmptyState";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { useDigest } from "@/hooks/useDigest";
import { formatDateTime } from "@/lib/format";
import type { DigestCategorySection } from "@/lib/types";

/** Digest detail (prompt.md §9.9): per-category sections (name → AI summary → source links),
 *  sources footer, fetch-failure warning, and a raw-fallback note when AI was unavailable. */
export function DigestDetail() {
  const { id } = useParams();
  const digest = useDigest(Number(id));

  return (
    <div className="space-y-4">
      <Link to="/digests" className="inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground">
        <ArrowLeft className="size-4" /> Back to digests
      </Link>

      {digest.isLoading ? (
        <div className="flex justify-center py-10"><Spinner className="size-6" /></div>
      ) : digest.isError ? (
        <ErrorBanner error={digest.error} />
      ) : digest.data ? (
        <Body payload={digest.data.payload} createdAt={digest.data.created_at} itemCount={digest.data.item_count} />
      ) : null}
    </div>
  );
}

function Body({
  payload,
  createdAt,
  itemCount,
}: {
  payload: import("@/lib/types").DigestPayload | null;
  createdAt: string;
  itemCount: number;
}) {
  if (!payload) {
    return <EmptyState title="Digest unavailable" description="This digest has no stored content." />;
  }

  return (
    <div className="space-y-4">
      <header>
        <h1 className="font-display text-2xl font-semibold tracking-tight">Digest — {formatDateTime(createdAt)}</h1>
        <p className="text-sm text-muted-foreground">
          {itemCount} item{itemCount === 1 ? "" : "s"} · {formatDateTime(payload.period_start)} → {formatDateTime(payload.period_end)}
        </p>
      </header>

      {payload.failure_warning && (
        <Alert variant="destructive">
          <AlertTriangle className="size-4" />
          {payload.failure_warning}
        </Alert>
      )}
      {payload.fallback_note && <Alert>{payload.fallback_note}</Alert>}

      {payload.categories.length === 0 ? (
        <EmptyState title="Nothing new" description="No items were published in this digest's window." />
      ) : (
        payload.categories.map((section) => <Section key={section.name} section={section} />)
      )}

      {payload.sources.length > 0 && (
        <footer className="border-t border-border pt-3 text-xs text-muted-foreground">
          <span className="font-medium">Sources:</span> {payload.sources.join(" · ")}
        </footer>
      )}
    </div>
  );
}

function Section({ section }: { section: DigestCategorySection }) {
  return (
    <Card>
      <CardHeader className="flex-row items-center justify-between gap-2 pb-3">
        <CardTitle className="text-base">{section.name}</CardTitle>
        <div className="flex items-center gap-2">
          <Badge>{section.items.length}</Badge>
          {!section.raw && (
            <span className="inline-flex items-center gap-1 text-xs text-muted-foreground">
              <Sparkles className="size-3" /> AI
            </span>
          )}
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        {section.ai_summary ? (
          <p className="whitespace-pre-wrap text-sm">{section.ai_summary}</p>
        ) : (
          <p className="text-sm text-muted-foreground">Raw headlines (no AI summary):</p>
        )}
        <ul className="space-y-1 text-sm">
          {section.items.map((it, i) => (
            <li key={i} className="flex gap-2">
              <span className="text-muted-foreground">·</span>
              {it.url ? (
                <a href={it.url} target="_blank" rel="noreferrer" className="text-primary hover:underline">
                  {it.title}
                </a>
              ) : (
                <span>{it.title}</span>
              )}
              <span className="text-xs text-muted-foreground">— {it.feed_title}</span>
            </li>
          ))}
        </ul>
      </CardContent>
    </Card>
  );
}
