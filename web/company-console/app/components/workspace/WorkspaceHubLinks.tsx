"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { cn } from "@/app/lib/utils";

const LINKS = [
  { href: "/workspace/dashboard", label: "Dashboard" },
  { href: "/workspace/issues", label: "Issues" },
  { href: "/workspace/agents", label: "Agents" },
  { href: "/workspace/intelligence", label: "Intelligence" },
  { href: "/workspace/approvals", label: "Approvals" },
  { href: "/workspace/costs", label: "Costs" },
  { href: "/workspace/my-work", label: "My work" },
  { href: "/workspace/playbooks", label: "Playbooks" },
  { href: "/workspace/marketplace", label: "Marketplace" },
  { href: "/workspace/graph", label: "Graph" },
  { href: "/workspace/architecture", label: "Architecture" },
] as const;

/**
 * Compact cross-links so every workspace surface can reach the rest of the graph without relying only on the sidebar.
 */
export function WorkspaceHubLinks() {
  const pathname = usePathname() ?? "";
  const { companies, postgresOk, companiesLoading } = useWorkspace();
  const needsWorkspace = postgresOk && !companiesLoading && companies.length === 0;

  return (
    <nav aria-label="Workspace sections" className="mb-4 space-y-2 border-b border-[#222222] pb-3">
      {needsWorkspace ? (
        <p className="rounded border border-[#D4A843]/40 bg-[#D4A843]/10 px-2 py-1.5 font-mono text-[11px] normal-case tracking-normal text-[#e8e8e8]">
          No company workspaces yet —{" "}
          <Link href="/workspace/marketplace" className="font-semibold text-[#79b8ff] underline-offset-2 hover:underline">
            open Marketplace
          </Link>{" "}
          to add one from the catalog.
        </p>
      ) : null}
      <div className="flex flex-wrap items-center gap-x-1 gap-y-1 font-mono text-[10px] uppercase tracking-[0.06em] text-[#666666]">
      {LINKS.map(({ href, label }, i) => {
        const active =
          pathname === href ||
          (href !== "/workspace/dashboard" && pathname.startsWith(href + "/")) ||
          (href === "/workspace/agents" && /^\/workspace\/agents\//i.test(pathname));
        return (
          <span key={href} className="inline-flex items-center">
            {i > 0 ? (
              <span className="mx-0.5 text-[#333333]" aria-hidden>
                ·
              </span>
            ) : null}
            <Link
              href={href}
              className={cn(
                "rounded px-1.5 py-0.5 transition-colors",
                active ? "bg-white text-black" : "text-[#999999] hover:bg-[#1a1a1a] hover:text-[#e8e8e8]",
              )}
            >
              {label}
            </Link>
          </span>
        );
      })}
      <span className="text-[#333333]">—</span>
      <Link
        href="/"
        className="rounded px-1.5 py-0.5 text-[#666666] hover:bg-[#1a1a1a] hover:text-[#e8e8e8]"
      >
        Legacy console
      </Link>
      </div>
    </nav>
  );
}
