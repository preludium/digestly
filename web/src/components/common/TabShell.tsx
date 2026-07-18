import { useSearchParams } from "react-router-dom";
import { PageTitle } from "@/components/common/PageHeadings";
import { cn } from "@/lib/utils";

export interface TabSection<Id extends string> {
    id: Id;
    label: string;
    description: string;
}

function navBtn(active: boolean): string {
    return cn(
        "shrink-0 whitespace-nowrap rounded-md px-3 py-1.5 text-sm font-semibold transition-colors hover:cursor-pointer",
        active
            ? "bg-primary/10 text-primary"
            : "text-muted-foreground hover:bg-muted hover:text-foreground",
    );
}

export function TabShell<Id extends string>({
    title,
    sections,
    initial,
    children,
}: {
    title: string;
    sections: readonly TabSection<Id>[];
    initial: Id;
    children: (section: Id) => React.ReactNode;
}) {
    const [params, setParams] = useSearchParams();
    const active =
        sections.find((s) => s.id === params.get("tab")) ??
        sections.find((s) => s.id === initial);

    if (!active) return null;
    const section = active.id;

    const selectSection = (id: Id) => {
        const next = new URLSearchParams(params);
        next.set("tab", id);
        setParams(next);
    };

    return (
        <div className="space-y-6">
            <PageTitle>{title}</PageTitle>
            <nav className="flex flex-wrap items-center gap-1 border-b border-border pb-3">
                {sections.map((s) => (
                    <button
                        key={s.id}
                        type="button"
                        onClick={() => selectSection(s.id)}
                        data-testid="tab-shell-tab"
                        data-tab-id={s.id}
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
                {children(section)}
            </section>
        </div>
    );
}
