"use client";

import { ReactNode } from "react";

export function Panel({
  title,
  children,
  className = "",
}: {
  title: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={`rounded border border-line bg-panel p-3 ${className}`}>
      <div className="mb-2 text-xs uppercase text-gray-500">{title}</div>
      {children}
    </div>
  );
}

