import type * as React from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

/** Centered card used by the login/register screens. */
export function AuthCard({
    title,
    children,
}: {
    title: string;
    children: React.ReactNode;
}) {
    return (
        <div className="flex min-h-dvh items-center justify-center p-4">
            <Card className="w-full max-w-sm">
                <CardHeader>
                    <span className="font-display text-center text-2xl font-semibold tracking-tight">
                        Digestly
                    </span>
                    <CardTitle className="text-center text-base font-medium text-muted-foreground">
                        {title}
                    </CardTitle>
                </CardHeader>
                <CardContent className="space-y-4">{children}</CardContent>
            </Card>
        </div>
    );
}

/** Inline validation message under a field. Rendered only once the field has been touched, so a
 *  pristine form doesn't open covered in "required" errors. */
export function FieldError({ message }: { message?: string }) {
    if (!message) return null;
    return <p className="text-xs text-destructive">{message}</p>;
}
