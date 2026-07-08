import { useState } from "react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { AiSettings } from "@/components/settings/AiSettings";
import { NotificationsSettings } from "@/components/settings/NotificationsSettings";
import { DigestSettings } from "@/components/settings/DigestSettings";
import { GeneralSettings } from "@/components/settings/GeneralSettings";
import { ImportExport } from "@/components/settings/ImportExport";
import { IngestionSettings } from "@/components/settings/IngestionSettings";
import { useMe } from "@/hooks/useAuth";

/** Settings (prompt.md §9.7) — tabbed; tabs shown depend on role. Per-user tabs (General,
 *  Notifications, Import/Export) for everyone; admin-only tabs (Ingestion, AI, Digest) gated both
 *  in the UI and server-side. */
export function Settings() {
  const { data: me } = useMe();
  const isAdmin = me?.role === "admin";
  const [tab, setTab] = useState("general");

  return (
    <div className="space-y-4">
      <h1 className="text-2xl font-bold">Settings</h1>
      <Tabs value={tab} onValueChange={setTab}>
        <TabsList>
          <TabsTrigger value="general">General</TabsTrigger>
          <TabsTrigger value="notifications">Notifications</TabsTrigger>
          <TabsTrigger value="import">Import / Export</TabsTrigger>
          {isAdmin && <TabsTrigger value="ingestion">Ingestion</TabsTrigger>}
          {isAdmin && <TabsTrigger value="ai">AI</TabsTrigger>}
          {isAdmin && <TabsTrigger value="digest">Digest</TabsTrigger>}
        </TabsList>

        <TabsContent value="general">
          <GeneralSettings />
        </TabsContent>
        <TabsContent value="notifications">
          <NotificationsSettings />
        </TabsContent>
        <TabsContent value="import">
          <ImportExport />
        </TabsContent>
        {isAdmin && (
          <TabsContent value="ingestion">
            <IngestionSettings />
          </TabsContent>
        )}
        {isAdmin && (
          <TabsContent value="ai">
            <AiSettings />
          </TabsContent>
        )}
        {isAdmin && (
          <TabsContent value="digest">
            <DigestSettings />
          </TabsContent>
        )}
      </Tabs>
    </div>
  );
}
