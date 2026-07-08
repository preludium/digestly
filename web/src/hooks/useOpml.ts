import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api";
import type { OpmlImportItem, OpmlImportResult, OpmlPreviewEntry } from "@/lib/types";

// OPML import/export (prompt.md §9.5, §9.7).

export function useOpmlPreview() {
  return useMutation({
    mutationFn: (opml: string) => api.post<{ entries: OpmlPreviewEntry[] }>("/opml/import", { opml }),
  });
}

export function useOpmlImport() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (items: OpmlImportItem[]) => api.post<OpmlImportResult>("/opml/import", { items }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["feeds"] });
      qc.invalidateQueries({ queryKey: ["categories"] });
    },
  });
}

/** Trigger an OPML file download from the export endpoint (sends the session cookie). */
export async function downloadOpmlExport() {
  const res = await fetch("/api/opml/export", { credentials: "include" });
  if (!res.ok) throw new Error("Export failed");
  const blob = await res.blob();
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = "digestly.opml";
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}
