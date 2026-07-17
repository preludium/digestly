import { TabShell } from "@/components/common/TabShell";
import { GeneralSettings } from "@/components/settings/GeneralSettings";
import { ImportExport } from "@/components/settings/ImportExport";
import { NotificationsSettings } from "@/components/settings/NotificationsSettings";

const SECTIONS = [
    {
        id: "general",
        label: "General",
        description: "Appearance, timezone, and reading defaults.",
    },
    {
        id: "notifications",
        label: "Notifications",
        description: "Push notifications for new digests.",
    },
    {
        id: "import",
        label: "Import / Export",
        description: "Move your feeds in and out via OPML.",
    },
] as const;

type SectionId = (typeof SECTIONS)[number]["id"];

export function Settings() {
    return (
        <TabShell<SectionId>
            title="Settings"
            sections={SECTIONS}
            initial="general"
        >
            {(section) => (
                <>
                    {section === "general" && <GeneralSettings />}
                    {section === "notifications" && <NotificationsSettings />}
                    {section === "import" && <ImportExport />}
                </>
            )}
        </TabShell>
    );
}
