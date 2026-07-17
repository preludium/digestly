import { cn } from "@/lib/utils";

export const SECTION_HEADER_CLASS =
    "border-b border-border pb-2 text-[13px] font-bold tracking-wide";

export function SectionHeader({
    children,
    className,
}: {
    children: React.ReactNode;
    className?: string;
}) {
    return <h3 className={cn(SECTION_HEADER_CLASS, className)}>{children}</h3>;
}

export function PageTitle({ children }: { children: React.ReactNode }) {
    return (
        <h1 className="font-display text-2xl font-semibold tracking-tight">
            {children}
        </h1>
    );
}
