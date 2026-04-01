"use client";

import { ReactNode } from "react";

export function Panel({
  title,
  children,
  className = "",
  variant = "default",
}: {
  title: string;
  children: ReactNode;
  className?: string;
  /** `console` — GitHub-dark / Paperclip-adjacent panels for Company OS inbox. */
  variant?: "default" | "console";
}) {
  if (variant === "console") {
    return (
      <div className={`rounded-2xl border border-[#30363D] bg-[#0d1117] p-4 ${className}`}>
        <div className="mb-3 font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-[#8B949E]">
          {title}
        </div>
        {children}
      </div>
    );
  }
  return (
    <div className={`rounded border border-line bg-panel p-3 ${className}`}>
      <div className="mb-2 text-xs uppercase text-gray-500">{title}</div>
      {children}
    </div>
  );
}

