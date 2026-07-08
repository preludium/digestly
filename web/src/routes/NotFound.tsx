import { Link } from "react-router-dom";
import { Button } from "@/components/ui/button";

/** 404 page (prompt.md §9.14). */
export function NotFound() {
  return (
    <div className="flex min-h-dvh flex-col items-center justify-center gap-4 p-6 text-center">
      <h1 className="text-3xl font-bold">404</h1>
      <p className="text-sm text-muted-foreground">This page doesn't exist.</p>
      <Button asChild>
        <Link to="/">Back to Digestly</Link>
      </Button>
    </div>
  );
}
