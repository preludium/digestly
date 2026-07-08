import * as React from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

/** Centered card used by the login/register screens. */
export function AuthCard({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="flex min-h-dvh items-center justify-center p-4">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <span className="text-center text-2xl font-bold tracking-tight">Digestly</span>
          <CardTitle className="text-center text-base font-medium text-muted-foreground">
            {title}
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">{children}</CardContent>
      </Card>
    </div>
  );
}

/** Renders TanStack Form field errors (validators return strings). */
export function FieldErrors({ errors }: { errors: unknown[] }) {
  const msgs = errors.filter(Boolean).map((e) => (typeof e === "string" ? e : String(e)));
  if (msgs.length === 0) return null;
  return <p className="text-xs text-destructive">{msgs.join(", ")}</p>;
}
