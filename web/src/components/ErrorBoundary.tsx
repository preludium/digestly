import * as React from "react";
import { Button } from "@/components/ui/button";

interface State {
    hasError: boolean;
}

/** App-wide React error boundary (prompt.md §9.14): crash → friendly fallback + reload. */
export class ErrorBoundary extends React.Component<
    React.PropsWithChildren,
    State
> {
    state: State = { hasError: false };

    static getDerivedStateFromError(): State {
        return { hasError: true };
    }

    componentDidCatch(error: unknown) {
        console.error("Unhandled UI error:", error);
    }

    render() {
        if (this.state.hasError) {
            return (
                <div className="flex min-h-dvh flex-col items-center justify-center gap-4 p-6 text-center">
                    <h1 className="text-xl font-semibold">Something broke</h1>
                    <p className="text-sm text-muted-foreground">
                        The app hit an unexpected error.
                    </p>
                    <Button onClick={() => window.location.reload()}>
                        Reload
                    </Button>
                </div>
            );
        }
        return this.props.children;
    }
}
