import { useState } from "react";
import { AiSettings } from "@/components/settings/AiSettings";
import { DigestSettings } from "@/components/settings/DigestSettings";
import { IngestionSettings } from "@/components/settings/IngestionSettings";
import { cn } from "@/lib/utils";

const SECTIONS = [
    {
        id: "ingestion",
        label: "Ingestion",
        description: "How and when feeds are fetched.",
    },
    {
        id: "ai",
        label: "AI",
        description: "Summary providers, models, and budgets.",
    },
    {
        id: "digest",
        label: "Digest",
        description: "Daily digest schedule and scope.",
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

/** Instance-wide admin configuration (prompt.md §8) - ingestion, AI, and the digest engine.
 *  Distinct from the per-user Settings page; only reachable via the sidebar's Admin group and
 *  gated at the route level (App.tsx) same as Users. */
export function System() {
    const [section, setSection] = useState<SectionId>("ingestion");
    const active = SECTIONS.find((s) => s.id === section)!;

    return (
        <div className="space-y-6">
            <h1 className="font-display text-2xl font-semibold tracking-tight">
                System
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
                {section === "ingestion" && <IngestionSettings />}
                {section === "ai" && <AiSettings />}
                {section === "digest" && <DigestSettings />}
            </section>
        </div>
    );
}
