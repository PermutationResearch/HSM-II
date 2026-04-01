"use client";

import { ReactNode } from "react";

export function ActionBar({ children }: { children: ReactNode }) {
  return (
    <div className="mb-4 flex flex-wrap items-center gap-2 rounded border border-line bg-panel p-2">
      {children}
    </div>
  );
}

