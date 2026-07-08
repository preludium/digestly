import { AlertCircle } from "lucide-react";
import { Alert } from "@/components/ui/alert";

/** Shared error banner (prompt.md §9 error states). Accepts an unknown error. */
export function ErrorBanner({ error }: { error: unknown }) {
  const message =
    error instanceof Error ? error.message : typeof error === "string" ? error : "Something went wrong";
  return (
    <Alert variant="destructive">
      <AlertCircle className="size-4" />
      {message}
    </Alert>
  );
}
