import { TabShell } from "@/components/common/TabShell";
import { AiSettings } from "@/components/settings/AiSettings";
import { DigestSettings } from "@/components/settings/DigestSettings";
import { IngestionSettings } from "@/components/settings/IngestionSettings";

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

/** Instance-wide admin configuration (prompt.md §8) - ingestion, AI, and the digest engine.
 *  Distinct from the per-user Settings page; only reachable via the sidebar's Admin group and
 *  gated at the route level (App.tsx) same as Users. */
export function System() {
    return (
        <TabShell<SectionId>
            title="System"
            sections={SECTIONS}
            initial="ingestion"
        >
            {(section) => (
                <>
                    {section === "ingestion" && <IngestionSettings />}
                    {section === "ai" && <AiSettings />}
                    {section === "digest" && <DigestSettings />}
                </>
            )}
        </TabShell>
    );
}
