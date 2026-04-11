"use client";

import Link from "next/link";
import { usePathname, useSearchParams } from "next/navigation";
import { Suspense, useMemo, useState } from "react";
import { Bot, ChevronDown, LayoutList } from "lucide-react";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { useCompanyAgents, useCompanyProjects } from "@/app/lib/hsm-queries";
import { cn } from "@/app/lib/utils";

const PROJECT_DOT = ["#a78bfa", "#60a5fa", "#34d399", "#f97316", "#818cf8", "#fb7185"];

function humanizeAgentTitle(name: string): string {
  return name
    .replace(/_/g, " ")
    .split(/\s+/)
    .filter(Boolean)
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1).toLowerCase())
    .join(" ");
}

function WorkspaceProjectsNavBody() {
  const pathname = usePathname() ?? "";
  const searchParams = useSearchParams();
  const selectedProject = searchParams.get("project") ?? "";
  const { apiBase, companyId } = useWorkspace();
  const { data: projectsRaw = [], isLoading: projLoading, isError: projError } = useCompanyProjects(
    apiBase,
    companyId,
  );
  const { data: agentsRaw = [], isLoading: agLoading } = useCompanyAgents(apiBase, companyId);

  const [projectsOpen, setProjectsOpen] = useState(true);
  const [agentsOpen, setAgentsOpen] = useState(true);

  const projects = useMemo(
    () =>
      [...projectsRaw]
        .filter((p) => (p.status ?? "active").toLowerCase() !== "archived")
        .sort((a, b) => {
          const o = (a.sort_order ?? 0) - (b.sort_order ?? 0);
          return o !== 0 ? o : a.title.localeCompare(b.title);
        })
        .slice(0, 24),
    [projectsRaw],
  );

  const agents = useMemo(
    () =>
      agentsRaw
        .filter((a) => (a.status ?? "").toLowerCase() !== "terminated")
        .sort((a, b) => a.name.localeCompare(b.name))
        .slice(0, 32),
    [agentsRaw],
  );

  if (!companyId) return null;

  return (
    <>
      <button
        type="button"
        className="mb-1 mt-1 flex w-full items-center justify-between rounded-md px-1 py-1 font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-[#888888] hover:bg-white/[0.04]"
        onClick={() => setProjectsOpen((o) => !o)}
      >
        <span>Projects</span>
        <ChevronDown className={cn("h-3.5 w-3.5 transition-transform", projectsOpen && "rotate-180")} />
      </button>
      {projectsOpen ? (
        <div className="mb-2 space-y-0.5">
          <Link
            href="/workspace/issues"
            title="All issues for this company"
            className={cn(
              "flex w-full items-center gap-2 rounded-md border border-transparent px-2 py-1.5 text-left text-[12px] transition-colors",
              pathname.startsWith("/workspace/issues") && !selectedProject
                ? "border-[#333333] bg-white/[0.05] text-white"
                : "text-[#b0b0b0] hover:bg-white/[0.06] hover:text-[#e8e8e8]",
            )}
          >
            <LayoutList className="h-3.5 w-3.5 shrink-0 opacity-80" strokeWidth={1.5} />
            <span className="min-w-0 flex-1 truncate">All issues</span>
          </Link>
          {projLoading ? (
            <p className="px-2 py-1 font-mono text-[10px] text-[#666666]">Loading…</p>
          ) : projError ? (
            <p className="px-2 py-1 font-mono text-[10px] leading-snug text-[#D4A843]">Projects unavailable</p>
          ) : projects.length === 0 ? (
            <p className="px-2 py-1 font-mono text-[10px] leading-snug text-[#666666]">
              None yet. Add a project from the legacy console, then attach issues to it.
            </p>
          ) : (
            projects.map((p, i) => {
              const href = `/workspace/issues?project=${encodeURIComponent(p.id)}`;
              const active = pathname.startsWith("/workspace/issues") && selectedProject === p.id;
              return (
                <Link
                  key={p.id}
                  href={href}
                  title={p.title}
                  className={cn(
                    "flex w-full items-center gap-2 rounded-md border border-transparent px-2 py-1.5 pl-3 text-left text-[12px] transition-colors",
                    active
                      ? "border-[#333333] bg-white/[0.05] text-white"
                      : "text-[#b0b0b0] hover:bg-white/[0.06] hover:text-[#e8e8e8]",
                  )}
                >
                  <span
                    className="h-2 w-2 shrink-0 rounded-full"
                    style={{ backgroundColor: PROJECT_DOT[i % PROJECT_DOT.length] }}
                  />
                  <span className="min-w-0 flex-1 truncate">{p.title}</span>
                </Link>
              );
            })
          )}
        </div>
      ) : null}

      <button
        type="button"
        className="mb-1 mt-2 flex w-full items-center justify-between rounded-md px-1 py-1 font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-[#888888] hover:bg-white/[0.04]"
        title="Open an agent’s Workspace tab: task ids, pack files, and work history"
        onClick={() => setAgentsOpen((o) => !o)}
      >
        <span>Agent workspaces</span>
        <ChevronDown className={cn("h-3.5 w-3.5 transition-transform", agentsOpen && "rotate-180")} />
      </button>
      {agentsOpen ? (
        <div className="space-y-0.5">
          {agLoading ? (
            <p className="px-2 py-1 font-mono text-[10px] text-[#666666]">Loading…</p>
          ) : agents.length === 0 ? (
            <p className="px-2 py-1 font-mono text-[10px] leading-snug text-[#666666]">
              No agents in roster. Open <span className="text-[#a0a0a0]">Agents</span> to add one.
            </p>
          ) : (
            agents.map((a) => {
              const href = `/workspace/agents/${a.id}?tab=workspace`;
              const onAgent =
                pathname.match(/^\/workspace\/agents\/([0-9a-fA-F-]{36})\/?$/i)?.[1] === a.id;
              const tab = searchParams.get("tab") ?? "workspace";
              const active = onAgent && tab === "workspace";
              const label = a.title?.trim() || humanizeAgentTitle(a.name);
              return (
                <Link
                  key={a.id}
                  href={href}
                  title={`${label} — tasks, files, and run status`}
                  className={cn(
                    "flex w-full items-center gap-2 rounded-md border border-transparent px-2 py-1.5 text-left text-[12px] transition-colors",
                    active
                      ? "border-[#333333] bg-white/[0.05] text-white"
                      : "text-[#b0b0b0] hover:bg-white/[0.06] hover:text-[#e8e8e8]",
                  )}
                >
                  <Bot className={cn("h-3.5 w-3.5 shrink-0", active ? "text-white" : "text-[#666666]")} />
                  <span className="min-w-0 flex-1 truncate font-mono text-[11px]">{label}</span>
                </Link>
              );
            })
          )}
        </div>
      ) : null}
    </>
  );
}

export function WorkspaceProjectsNav() {
  return (
    <Suspense
      fallback={
        <div className="mt-1 space-y-2 px-1">
          <div className="nd-ws-nav-section-label">Projects</div>
          <p className="px-2 font-mono text-[10px] text-[#666666]">Loading…</p>
        </div>
      }
    >
      <WorkspaceProjectsNavBody />
    </Suspense>
  );
}
