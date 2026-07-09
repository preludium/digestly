import { useState } from "react";
import { cn } from "@/lib/utils";
import { AiSettings } from "@/components/settings/AiSettings";
import { NotificationsSettings } from "@/components/settings/NotificationsSettings";
import { DigestSettings } from "@/components/settings/DigestSettings";
import { GeneralSettings } from "@/components/settings/GeneralSettings";
import { ImportExport } from "@/components/settings/ImportExport";
import { IngestionSettings } from "@/components/settings/IngestionSettings";
import { useMe } from "@/hooks/useAuth";

const SECTIONS = [
  { id: "general", label: "General", description: "Appearance, timezone, and reading defaults." },
  { id: "notifications", label: "Notifications", description: "Push notifications for new digests." },
  { id: "import", label: "Import / Export", description: "Move your feeds in and out via OPML." },
] as const;

const ADMIN_SECTIONS = [
  { id: "ingestion", label: "Ingestion", description: "How and when feeds are fetched." },
  { id: "ai", label: "AI", description: "Summary providers, models, and budgets." },
  { id: "digest", label: "Digest", description: "Daily digest schedule and scope." },
] as const;

type SectionId = (typeof SECTIONS)[number]["id"] | (typeof ADMIN_SECTIONS)[number]["id"];

export function Settings() {
  const { data: me } = useMe();
  const isAdmin = me?.role === "admin";
  const [section, setSection] = useState<SectionId>("general");

  const allSections = isAdmin ? [...SECTIONS, ...ADMIN_SECTIONS] : [...SECTIONS];
  const active = allSections.find((s) => s.id === section)!;

  return (
    <div className="space-y-6">
      <h1 className="font-display text-2xl font-semibold tracking-tight">Settings</h1>

      <div className="grid gap-6 md:grid-cols-[200px_1fr]">
        <nav className="flex flex-row gap-1 overflow-x-auto md:flex-col">
          {SECTIONS.map((s) => (
            <button
              key={s.id}
              type="button"
              onClick={() => setSection(s.id)}
              className={cn(
                "shrink-0 rounded-md px-3 py-2 text-left text-sm font-medium transition-colors",
                section === s.id
                  ? "bg-primary/10 text-primary"
                  : "text-muted-foreground hover:bg-muted hover:text-foreground",
              )}
            >
              {s.label}
            </button>
          ))}
          {isAdmin && (
            <>
              <p className="mt-3 px-3 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                Admin
              </p>
              {ADMIN_SECTIONS.map((s) => (
                <button
                  key={s.id}
                  type="button"
                  onClick={() => setSection(s.id)}
                  className={cn(
                    "shrink-0 rounded-md px-3 py-2 text-left text-sm font-medium transition-colors",
                    section === s.id
                      ? "bg-primary/10 text-primary"
                      : "text-muted-foreground hover:bg-muted hover:text-foreground",
                  )}
                >
                  {s.label}
                </button>
              ))}
            </>
          )}
        </nav>

        <section className="min-w-0 space-y-4">
          <div>
            <h2 className="font-display text-lg font-semibold tracking-tight">{active.label}</h2>
            <p className="text-sm text-muted-foreground">{active.description}</p>
          </div>
          {section === "general" && <GeneralSettings />}
          {section === "notifications" && <NotificationsSettings />}
          {section === "import" && <ImportExport />}
          {section === "ingestion" && <IngestionSettings />}
          {section === "ai" && <AiSettings />}
          {section === "digest" && <DigestSettings />}
        </section>
      </div>
    </div>
  );
}
