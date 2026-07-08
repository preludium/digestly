import { useRef, useState } from "react";
import { Download, Upload } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { ConnectedAccounts } from "@/components/settings/ConnectedAccounts";
import { downloadOpmlExport, useOpmlImport, useOpmlPreview } from "@/hooks/useOpml";
import { toast } from "@/stores/toast";
import type { OpmlImportItem, OpmlPreviewEntry } from "@/lib/types";

type Row = OpmlPreviewEntry & { category_edit: string; include: boolean };

/** Import/Export tab (prompt.md §9.7): OPML upload → preview → confirm (each feed needs a category,
 *  default Other) and a one-click export download. Lossless round-trip. */
export function ImportExport() {
  const preview = useOpmlPreview();
  const doImport = useOpmlImport();
  const fileRef = useRef<HTMLInputElement>(null);
  const [rows, setRows] = useState<Row[] | null>(null);

  const onFile = async (file: File) => {
    const text = await file.text();
    preview.mutate(text, {
      onSuccess: (r) =>
        setRows(
          r.entries.map((e) => ({ ...e, category_edit: e.category ?? "Other", include: !e.already_subscribed })),
        ),
      onError: (e) => toast(e instanceof Error ? e.message : "Could not read OPML", "error"),
    });
  };

  const confirm = () => {
    if (!rows) return;
    const items: OpmlImportItem[] = rows
      .filter((r) => r.include)
      .map((r) => ({ feed_url: r.feed_url, title: r.title, kind: r.kind, category: r.category_edit.trim() || "Other" }));
    if (items.length === 0) {
      toast("Select at least one feed to import", "error");
      return;
    }
    doImport.mutate(items, {
      onSuccess: (res) => {
        toast(`Imported ${res.imported}, skipped ${res.skipped}`, "success");
        setRows(null);
        if (fileRef.current) fileRef.current.value = "";
      },
      onError: (e) => toast(e instanceof Error ? e.message : "Import failed", "error"),
    });
  };

  const doExport = () =>
    downloadOpmlExport().catch((e) => toast(e instanceof Error ? e.message : "Export failed", "error"));

  return (
    <div className="space-y-6">
      <ConnectedAccounts />

      <section className="space-y-2">
        <h2 className="text-base font-semibold">Export</h2>
        <p className="text-sm text-muted-foreground">Download all your subscriptions as an OPML file, grouped by category.</p>
        <Button variant="outline" onClick={doExport}>
          <Download className="size-4" /> Export OPML
        </Button>
      </section>

      <section className="space-y-3">
        <h2 className="text-base font-semibold">Import</h2>
        <p className="text-sm text-muted-foreground">Upload an OPML file, review the feeds, assign categories, then import.</p>
        <input
          ref={fileRef}
          type="file"
          accept=".opml,.xml,text/xml,application/xml"
          className="hidden"
          onChange={(e) => e.target.files?.[0] && onFile(e.target.files[0])}
        />
        <Button variant="outline" disabled={preview.isPending} onClick={() => fileRef.current?.click()}>
          <Upload className="size-4" /> {preview.isPending ? "Reading…" : "Choose OPML file"}
        </Button>
        {preview.isError && <ErrorBanner error={preview.error} />}

        {rows && (
          <div className="space-y-2">
            <ul className="divide-y divide-border rounded-md border border-border">
              {rows.map((r, i) => (
                <li key={r.feed_url} className="flex flex-col gap-2 p-3 sm:flex-row sm:items-center">
                  <label className="flex min-w-0 flex-1 items-center gap-2">
                    <input
                      type="checkbox"
                      className="size-4 accent-primary"
                      checked={r.include}
                      onChange={(e) => setRows((rs) => rs!.map((x, j) => (j === i ? { ...x, include: e.target.checked } : x)))}
                    />
                    <span className="min-w-0">
                      <span className="block truncate text-sm font-medium">{r.title ?? r.feed_url}</span>
                      <span className="block truncate text-xs text-muted-foreground">{r.feed_url}</span>
                    </span>
                    {r.already_subscribed && <Badge>subscribed</Badge>}
                  </label>
                  <Input
                    className="sm:w-48"
                    value={r.category_edit}
                    aria-label="Category"
                    onChange={(e) => setRows((rs) => rs!.map((x, j) => (j === i ? { ...x, category_edit: e.target.value } : x)))}
                  />
                </li>
              ))}
            </ul>
            <div className="flex justify-end gap-2">
              <Button variant="ghost" onClick={() => setRows(null)}>
                Cancel
              </Button>
              <Button onClick={confirm} disabled={doImport.isPending}>
                {doImport.isPending ? "Importing…" : "Import selected"}
              </Button>
            </div>
          </div>
        )}
      </section>
    </div>
  );
}
