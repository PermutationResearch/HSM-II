"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { cn } from "@/app/lib/utils";

type HubLink = { href: string; label: string; hint: string };

type HubGroup = { id: string; label: string; blurb: string; links: HubLink[] };

const GROUPS: HubGroup[] = [
  {
    id: "work",
    label: "Work",
    blurb: "Triage tasks and human gates",
    links: [
      { href: "/workspace/dashboard", label: "Dashboard", hint: "Company overview, counts, and shortcuts into queues" },
      { href: "/workspace/issues", label: "Issues", hint: "All tasks — filter by state, owner, or priority" },
      { href: "/workspace/approvals", label: "Approvals", hint: "Items waiting on policy or human decision" },
      { href: "/workspace/my-work", label: "My work", hint: "Tasks relevant to you" },
    ],
  },
  {
    id: "team",
    label: "Team & assets",
    blurb: "Agents, SOPs, and packs",
    links: [
      { href: "/workspace/agents", label: "Agents", hint: "Roster agents, workspace files, memory, skills" },
      { href: "/workspace/playbooks", label: "Playbooks", hint: "Author SOPs and implement as tasks" },
      { href: "/workspace/marketplace", label: "Marketplace", hint: "Add or install company packs from catalog" },
    ],
  },
  {
    id: "insights",
    label: "Insights",
    blurb: "Signals, spend, and structure",
    links: [
      { href: "/workspace/intelligence", label: "Intelligence", hint: "Goals, workflow feed, and runtime summary" },
      { href: "/workspace/costs", label: "Costs", hint: "Spend rollups for the company" },
      { href: "/workspace/graph", label: "Graph", hint: "Hypergraph / trail views" },
      { href: "/workspace/architecture", label: "Architecture", hint: "HSM-II blueprint (RON); company ops live in the pack" },
    ],
  },
];

function linkActive(pathname: string, href: string): boolean {
  if (pathname === href) return true;
  if (href === "/workspace/dashboard") return false;
  if (href === "/workspace/agents" && /^\/workspace\/agents\//i.test(pathname)) return true;
  return pathname.startsWith(href + "/");
}

/**
 * Grouped cross-links so every workspace surface can reach the rest without hunting the sidebar.
 */
export function WorkspaceHubLinks() {
  const pathname = usePathname() ?? "";
  const { companies, postgresOk, companiesLoading } = useWorkspace();
  const needsWorkspace = postgresOk && !companiesLoading && companies.length === 0;

  return (
    <nav aria-label="Workspace sections" className="mb-5 space-y-3 border-b border-[#222222] pb-4">
      {needsWorkspace ? (
        <p className="rounded border border-[#D4A843]/40 bg-[#D4A843]/10 px-3 py-2 font-mono text-[11px] normal-case tracking-normal text-[#e8e8e8]">
          No company workspaces yet —{" "}
          <Link href="/workspace/marketplace" className="font-semibold text-[#79b8ff] underline-offset-2 hover:underline">
            open Marketplace
          </Link>{" "}
          to add one from the catalog.
        </p>
      ) : null}

      <div className="grid gap-3 sm:grid-cols-3">
        {GROUPS.map((g) => (
          <div
            key={g.id}
            className="rounded-lg border border-[#2a2a2a] bg-[#0d0d0d]/80 px-3 py-2.5"
          >
            <p className="font-mono text-[9px] font-semibold uppercase tracking-[0.12em] text-[#666666]">{g.label}</p>
            <p className="mt-0.5 text-[10px] leading-snug text-[#555555]">{g.blurb}</p>
            <ul className="mt-2 flex flex-col gap-1">
              {g.links.map((item) => {
                const active = linkActive(pathname, item.href);
                return (
                  <li key={item.href}>
                    <Link
                      href={item.href}
                      title={item.hint}
                      className={cn(
                        "block rounded-md px-2 py-1.5 font-sans text-[13px] transition-colors",
                        active
                          ? "bg-white text-black font-medium"
                          : "text-[#bbbbbb] hover:bg-white/[0.06] hover:text-white",
                      )}
                    >
                      {item.label}
                    </Link>
                  </li>
                );
              })}
            </ul>
          </div>
        ))}
      </div>

      <div className="flex flex-wrap items-center justify-end gap-2 border-t border-[#1a1a1a] pt-3">
        <span className="font-mono text-[10px] uppercase tracking-wide text-[#444444]">Other</span>
        <Link
          href="/"
          className="rounded-md px-2 py-1 font-mono text-[11px] text-[#888888] hover:bg-[#1a1a1a] hover:text-[#e8e8e8]"
        >
          Legacy console →
        </Link>
      </div>
    </nav>
  );
}
