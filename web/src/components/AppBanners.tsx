import { useEffect, useState } from "react";
import { WifiOff, RefreshCw } from "lucide-react";
import { applyServiceWorkerUpdate } from "@/lib/pwa";
import { useOutboxSync } from "@/lib/sync";

/** Global connectivity + update banners (prompt.md §9.14, §9a). The offline banner appears when the
 *  browser is offline (cached content still reads) and reports any read/star changes queued for
 *  sync; the update banner appears when a new service worker is waiting. */
export function AppBanners() {
  const [offline, setOffline] = useState(typeof navigator !== "undefined" && !navigator.onLine);
  const [updateReady, setUpdateReady] = useState(false);
  const pending = useOutboxSync();

  useEffect(() => {
    const on = () => setOffline(false);
    const off = () => setOffline(true);
    const upd = () => setUpdateReady(true);
    window.addEventListener("online", on);
    window.addEventListener("offline", off);
    window.addEventListener("hf-sw-update", upd);
    return () => {
      window.removeEventListener("online", on);
      window.removeEventListener("offline", off);
      window.removeEventListener("hf-sw-update", upd);
    };
  }, []);

  // When back online with a queue still draining, keep the user informed instead of going silent.
  const syncing = !offline && pending > 0;
  if (!offline && !updateReady && !syncing) return null;

  const changes = `${pending} change${pending === 1 ? "" : "s"}`;

  return (
    <div className="fixed inset-x-0 top-0 z-50 flex flex-col items-center gap-px">
      {offline && (
        <div className="flex w-full items-center justify-center gap-2 bg-muted px-3 py-1.5 text-xs font-medium text-muted-foreground">
          <WifiOff className="size-3.5" /> You’re offline — showing cached items.
          {pending > 0 && <span>· {changes} will sync when you reconnect.</span>}
        </div>
      )}
      {syncing && (
        <div className="flex w-full items-center justify-center gap-2 bg-muted px-3 py-1.5 text-xs font-medium text-muted-foreground">
          <RefreshCw className="size-3.5 animate-spin" /> Syncing {changes}…
        </div>
      )}
      {updateReady && (
        <button
          type="button"
          onClick={applyServiceWorkerUpdate}
          className="flex w-full items-center justify-center gap-2 bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground"
        >
          <RefreshCw className="size-3.5" /> A new version is available — tap to update.
        </button>
      )}
    </div>
  );
}
