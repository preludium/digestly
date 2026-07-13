import { Rocket } from "lucide-react";
import { EmptyState } from "@/components/common/EmptyState";

/** Placeholder for routes owned by later phases (clean stub, never a dead link). */
export function ComingSoon({ title }: { title: string }) {
    return (
        <div>
            <EmptyState
                icon={<Rocket className="size-8" />}
                title={title}
                description="This screen arrives in a later build phase."
            />
        </div>
    );
}
