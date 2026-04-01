import { timeAgo } from "./timeAgo";

/** Tailwind-friendly class merge (minimal). */
export function cn(...inputs: (string | false | null | undefined)[]): string {
  return inputs.filter(Boolean).join(" ");
}

/** Paperclip `relativeTime` — alias of HSM timeAgo. */
export function relativeTime(iso: string): string {
  return timeAgo(iso);
}

export function formatCents(cents: number): string {
  const neg = cents < 0;
  const v = Math.abs(Math.round(cents)) / 100;
  const s = v.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  return neg ? `-$${s}` : `$${s}`;
}
