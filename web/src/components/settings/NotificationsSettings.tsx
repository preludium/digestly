import { useEffect, useState } from "react";
import { Wifi } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Spinner } from "@/components/ui/spinner";
import { ErrorBanner } from "@/components/common/ErrorBanner";
import { useNotifications, useTestNotification, useUpdateNotifications } from "@/hooks/useNotifications";
import { toast } from "@/stores/toast";
import type { PutNotifications } from "@/lib/types";

/** Notifications (ntfy) tab (prompt.md §7a, §9.7) — every user configures their own push channel:
 *  server URL, topic, write-only auth token, priority, per-event toggles, and a Test button. */
export function NotificationsSettings() {
  const config = useNotifications();
  const update = useUpdateNotifications();
  const test = useTestNotification();

  const [form, setForm] = useState({
    ntfy_server_url: "",
    ntfy_topic: "",
    ntfy_priority: 3,
    notify_on_digest: true,
    notify_on_feed_health: true,
  });
  const [token, setToken] = useState("");
  const [hasToken, setHasToken] = useState(false);

  useEffect(() => {
    if (config.data) {
      const c = config.data;
      setForm({
        ntfy_server_url: c.ntfy_server_url ?? "",
        ntfy_topic: c.ntfy_topic ?? "",
        ntfy_priority: c.ntfy_priority,
        notify_on_digest: c.notify_on_digest,
        notify_on_feed_health: c.notify_on_feed_health,
      });
      setHasToken(c.has_token);
      setToken("");
    }
  }, [config.data]);

  if (config.isLoading) return <div className="flex justify-center py-6"><Spinner className="size-6" /></div>;
  if (config.isError) return <ErrorBanner error={config.error} />;

  const patch = (p: Partial<typeof form>) => setForm((f) => ({ ...f, ...p }));

  const save = () => {
    const body: PutNotifications = {
      ntfy_server_url: form.ntfy_server_url.trim() || null,
      ntfy_topic: form.ntfy_topic.trim() || null,
      ntfy_priority: form.ntfy_priority,
      notify_on_digest: form.notify_on_digest,
      notify_on_feed_health: form.notify_on_feed_health,
    };
    // Only send the token when the user typed a new one (write-only, never round-tripped).
    if (token.trim()) body.auth_token = token.trim();
    update.mutate(body, {
      onSuccess: () => {
        toast("Notification settings saved", "success");
        setToken("");
      },
      onError: (e) => toast(e instanceof Error ? e.message : "Could not save", "error"),
    });
  };

  const clearToken = () =>
    update.mutate(
      { auth_token: "" },
      {
        onSuccess: () => {
          toast("Auth token cleared");
          setHasToken(false);
        },
        onError: (e) => toast(e instanceof Error ? e.message : "Could not clear token", "error"),
      },
    );

  const runTest = () =>
    test.mutate(undefined, {
      onSuccess: (r) => toast(r.ok ? "Test push sent" : `Test failed: ${r.error ?? "unknown error"}`, r.ok ? "success" : "error"),
      onError: (e) => toast(e instanceof Error ? e.message : "Test failed", "error"),
    });

  return (
    <div className="space-y-5">
      <div>
        <h2 className="text-base font-semibold">Notifications (ntfy)</h2>
        <p className="text-sm text-muted-foreground">
          Digestly pushes to your own ntfy server (self-hosted or ntfy.sh). Enter your channel below.
        </p>
      </div>

      <div className="grid gap-4 sm:grid-cols-2">
        <div className="space-y-1.5">
          <Label htmlFor="ntfy-url">Server URL</Label>
          <Input
            id="ntfy-url"
            value={form.ntfy_server_url}
            onChange={(e) => patch({ ntfy_server_url: e.target.value })}
            placeholder="https://ntfy.sh"
          />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="ntfy-topic">Topic</Label>
          <Input
            id="ntfy-topic"
            value={form.ntfy_topic}
            onChange={(e) => patch({ ntfy_topic: e.target.value })}
            placeholder="my-digestly"
          />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="ntfy-token">Auth token (optional)</Label>
          <Input
            id="ntfy-token"
            type="password"
            value={token}
            onChange={(e) => setToken(e.target.value)}
            placeholder={hasToken ? "🔒 token saved · hidden" : "tk_… (for protected topics)"}
            autoComplete="off"
          />
          {hasToken && (
            <button type="button" className="text-xs text-muted-foreground underline" onClick={clearToken}>
              Clear saved token
            </button>
          )}
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="ntfy-priority">Priority</Label>
          <Select
            value={String(form.ntfy_priority)}
            onValueChange={(v) => patch({ ntfy_priority: Number(v) })}
          >
            <SelectTrigger id="ntfy-priority">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="1">Min (1)</SelectItem>
              <SelectItem value="2">Low (2)</SelectItem>
              <SelectItem value="3">Default (3)</SelectItem>
              <SelectItem value="4">High (4)</SelectItem>
              <SelectItem value="5">Max (5)</SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>

      <fieldset className="space-y-2">
        <legend className="text-sm font-medium">Notify me…</legend>
        <label className="flex items-center gap-2 text-sm">
          <Switch
            checked={form.notify_on_digest}
            onCheckedChange={(v) => patch({ notify_on_digest: v })}
          />
          After each digest
        </label>
        <label className="flex items-center gap-2 text-sm">
          <Switch
            checked={form.notify_on_feed_health}
            onCheckedChange={(v) => patch({ notify_on_feed_health: v })}
          />
          On feed health issues
        </label>
      </fieldset>

      <div className="flex items-center justify-end gap-2">
        <Button variant="outline" disabled={test.isPending} onClick={runTest}>
          {test.isPending ? <Spinner className="size-4" /> : <Wifi className="size-4" />} Test
        </Button>
        <Button onClick={save} disabled={update.isPending}>
          {update.isPending ? "Saving…" : "Save settings"}
        </Button>
      </div>
    </div>
  );
}
