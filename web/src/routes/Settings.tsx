import { useState } from "react";
import { GeneralSettings } from "@/components/settings/GeneralSettings";
import { ImportExport } from "@/components/settings/ImportExport";
import { NotificationsSettings } from "@/components/settings/NotificationsSettings";
import { cn } from "@/lib/utils";

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

function navBtn(active: boolean): string {
    return cn(
        "shrink-0 whitespace-nowrap rounded-md px-3 py-1.5 text-sm font-semibold transition-colors",
        active
            ? "bg-primary/10 text-primary"
            : "text-muted-foreground hover:bg-muted hover:text-foreground",
    );
}

export function Settings() {
    const [section, setSection] = useState<SectionId>("general");
    const active = SECTIONS.find((s) => s.id === section)!;

    return (
        <div className="space-y-6">
            <h1 className="font-display text-2xl font-semibold tracking-tight">
                Settings
            </h1>

            <nav className="flex flex-wrap items-center gap-1 border-b border-border pb-3">
                {SECTIONS.map((s) => (
                    <button
                        key={s.id}
                        type="button"
                        onClick={() => setSection(s.id)}
                        className={navBtn(section === s.id)}
                    >
                        {s.label}
                    </button>
                ))}
            </nav>

            <section className="min-w-0 space-y-4">
                <div>
                    <h2 className="font-display text-lg font-semibold tracking-tight">
                        {active.label}
                    </h2>
                    <p className="text-sm text-muted-foreground">
                        {active.description}
                    </p>
                </div>
                {section === "general" && <GeneralSettings />}
                {section === "notifications" && <NotificationsSettings />}
                {section === "import" && <ImportExport />}
            </section>
        </div>
    );
}
