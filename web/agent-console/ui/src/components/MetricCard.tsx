import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "../lib/utils";

/**
 * Paperclip MetricCard API — see
 * https://github.com/paperclipai/paperclip/blob/master/ui/src/components/MetricCard.tsx
 * `Link` replaced with `<a>` for HSM Next shell.
 */
interface MetricCardProps {
  icon: LucideIcon;
  value: string | number;
  label: string;
  description?: ReactNode;
  to?: string;
  onClick?: () => void;
  /** `admin` = Paperclip-style contrast and hover */
  variant?: "nothing" | "admin";
}

export function MetricCard({ icon: Icon, value, label, description, to, onClick, variant = "nothing" }: MetricCardProps) {
  const isClickable = !!(to || onClick);

  const inner = (
    <div
      className={cn(
        "rounded-2xl p-6 text-card-foreground transition-[background-color,border-color] duration-200 ease-out",
        variant === "admin"
          ? "border border-[#2a2a2a] bg-[#0a0a0a]"
          : "border border-border bg-card",
        isClickable &&
          (variant === "admin"
            ? "cursor-pointer hover:border-[#388bfd]/50 hover:bg-[#111111]"
            : "cursor-pointer hover:border-[#333333] hover:bg-[#1A1A1A]")
      )}
    >
      <div className="flex flex-row items-center justify-between space-y-0 pb-2">
        <h3
          className={
            variant === "admin"
              ? "font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-[#8B949E]"
              : "nd-label text-[11px]"
          }
        >
          {label}
        </h3>
        <Icon className="h-4 w-4 text-muted-foreground" strokeWidth={1.5} />
      </div>
      <div className="font-mono text-2xl font-normal tabular-nums tracking-tight text-white">{value}</div>
      {description ? <p className="mt-1 text-xs text-muted-foreground">{description}</p> : null}
    </div>
  );

  if (to) {
    return (
      <a href={to} className="block no-underline">
        {inner}
      </a>
    );
  }

  if (onClick) {
    return (
      <button type="button" onClick={onClick} className="block w-full text-left">
        {inner}
      </button>
    );
  }

  return inner;
}
