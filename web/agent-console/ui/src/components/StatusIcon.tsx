import { cn } from "../lib/utils";

/** Paperclip-style status dot — semantic colors. */
export function StatusIcon({ status }: { status: string }) {
  const s = status.toUpperCase();
  const cls =
    s.includes("BLOCK") || s.includes("FAIL")
      ? "bg-destructive"
      : s.includes("DONE") || s.includes("COMPLETE") || s.includes("CLOSE")
        ? "bg-emerald-500"
        : s.includes("PROGRESS") || s.includes("OPEN")
          ? "bg-accent"
          : "bg-muted-foreground";
  return <span className={cn("inline-block h-2.5 w-2.5 shrink-0 rounded-full", cls)} title={status} />;
}
