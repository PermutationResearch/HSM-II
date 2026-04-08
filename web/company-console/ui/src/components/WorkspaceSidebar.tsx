"use client";

import type { LucideIcon } from "lucide-react";
import {
  BarChart3,
  BookOpen,
  Bot,
  Building2,
  ChevronDown,
  Code2,
  Crown,
  Eye,
  Folder,
  Inbox,
  LayoutDashboard,
  LayoutList,
  Loader2,
  Lock,
  Package,
  Plus,
  ShieldCheck,
  Sparkles,
  Store,
  Trash2,
  TrendingUp,
  UserCog,
} from "lucide-react";
import type { ComponentType, ReactNode } from "react";
import { useMemo, useState } from "react";
import type { CompaniesShItem } from "../hooks/useCompaniesShCatalog";
import {
  companiesShInstallPath,
  findCompanyByPackFolder,
  findExistingCompanyForCatalogPack,
  isPaperclipPack,
  slugBaseFromCatalogItem,
} from "../hooks/useCompaniesShCatalog";
import { cn } from "../lib/utils";

/** Matches `NavId` in app/page.tsx — keep in sync when adding views */
export type WorkspaceConsoleView =
  | "dash"
  | "onboard"
  | "company"
  | "command"
  | "quality"
  | "trail"
  | "memory"
  | "graph"
  | "search"
  | "email"
  | "marketplace"
  | "sops";

export type WorkspaceSidebarProps = {
  workspaceLabel: string;
  workspaceInitial: string;
  companies: { id: string; display_name: string; slug: string; hsmii_home?: string | null }[];
  selectedCompanyId: string | null;
  onSelectCompany: (id: string) => void;
  view: WorkspaceConsoleView;
  onNavigate: (id: WorkspaceConsoleView) => void;
  dashboardLiveCount: number;
  inboxCount: number;
  /** When `view === "company"`, which workspace sub-tab is active (for sidebar highlights). */
  companyWorkTab?: "inbox" | "tasks" | "packs" | "sops" | "team" | "advanced" | null;
  /** Open Company OS on the human decision inbox. */
  onNavigateInbox?: () => void;
  /** Open Company OS on the full task list. */
  onNavigateTasks?: () => void;
  /** Paperclip-style task containers (distinct from strategic goals). */
  projects?: { id: string; title: string }[];
  selectedProjectId?: string | null;
  onSelectProject?: (projectId: string) => void;
  /** Company OS strategic goal tree (Advanced → Goals & governance). */
  goals: { id: string; title: string }[];
  /** `id` = persona string (task owner / checkout); `registryAgentId` when this row exists in workforce roster */
  agents: { id: string; name: string; liveCount: number; registryAgentId: string | null }[];
  /** Highlighted agent from sidebar — matches task `owner_persona` / `checked_out_by` */
  selectedAgentPersona?: string | null;
  /** Opens Tasks (and filters) scoped to that persona */
  onSelectAgent?: (persona: string) => void;
  /** DELETE roster row; only shown when `registryAgentId` is set (task-only names have no row to delete) */
  onDeleteRegistryAgent?: (registryAgentId: string, personaId: string) => void;
  /** Jump to workforce form (Team & setup tab) */
  onAddRegistryAgent?: () => void;
  onNewIssue: () => void;
  onOpenOnboarding?: () => void;
  /** Permanently remove a workspace from Company OS (tasks, agents, goals cascade). */
  onDeleteCompany?: (company: { id: string; slug: string; display_name: string }) => Promise<void>;
  apiBase: string;
  /** [companies.sh](https://companies.sh/) open directory — pre-seed Company OS workspaces */
  catalog?: {
    items: CompaniesShItem[];
    loading: boolean;
    error?: string | null;
  };
  onCreateFromCatalog?: (item: CompaniesShItem) => Promise<void>;
};

function NavButton({
  active,
  onClick,
  icon: Icon,
  label,
  badge,
  badgeVariant = "default",
  title: titleAttr,
}: {
  active: boolean;
  onClick: () => void;
  icon: ComponentType<{ className?: string }>;
  label: string;
  badge?: number;
  badgeVariant?: "default" | "live";
  title?: string;
}) {
  return (
    <button
      type="button"
      title={titleAttr}
      onClick={onClick}
      className={cn(
        "flex w-full items-center gap-2.5 rounded-full px-2.5 py-2 text-left text-sm font-mono uppercase tracking-[0.06em] transition-colors duration-200 ease-out",
        active ? "bg-white text-black" : "text-[#999999] hover:bg-[#1A1A1A] hover:text-[#E8E8E8]"
      )}
    >
      <Icon className="h-4 w-4 shrink-0 opacity-80" />
      <span className=" min-w-0 flex-1 truncate font-medium">{label}</span>
      {badge !== undefined && badge > 0 ? (
        <span
          className={cn(
            "shrink-0 rounded-md px-1.5 py-0.5 text-[10px] font-semibold tabular-nums",
            badgeVariant === "live"
              ? active
                ? "bg-[#4A9E5C] text-white"
                : "border border-[#4A9E5C]/50 bg-[#4A9E5C]/15 text-[#4A9E5C]"
              : active
                ? "bg-[#333333] text-white"
                : "bg-[#1A1A1A] text-[#999999]"
          )}
        >
          {badgeVariant === "live" ? `${badge} live` : String(badge)}
        </span>
      ) : null}
    </button>
  );
}

function SubLink({
  active,
  onClick,
  label,
  suffix,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
  suffix?: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex w-full items-center justify-between rounded-md px-2 py-1.5 pl-9 text-left font-mono text-[12px] font-normal uppercase tracking-[0.06em]",
        active ? "bg-[#1A1A1A] text-white" : "text-[#666666] hover:bg-[#111111] hover:text-[#999999]"
      )}
    >
      <span>{label}</span>
      {suffix}
    </button>
  );
}

function SectionTitle({ children }: { children: ReactNode }) {
  return (
    <div className="mb-1.5 mt-5 px-2 font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-[#666666]">
      {children}
    </div>
  );
}

/** Deterministic dot colors for goal rows — cycle by index */
const GOAL_ROW_DOT_COLORS = ["#a78bfa", "#60a5fa", "#f97316", "#818cf8", "#34d399", "#fb7185", "#fbbf24", "#38bdf8"];

/** Map agent name keywords → icon component */
function getAgentIcon(name: string): LucideIcon {
  const n = name.toLowerCase();
  if (n.includes("ceo") || n.includes("chief executive")) return Crown;
  if (n.includes("security") || n.includes("auditor")) return ShieldCheck;
  if (n.includes("qa") || n.includes("tester") || n.includes("quality")) return Eye;
  if (n.includes("oracle")) return Eye;
  if (n.includes("architect")) return Building2;
  if (n.includes("product") || n.includes("pm ") || n.startsWith("pm")) return Package;
  if (n.includes("knowledge") || n.includes("broker")) return BookOpen;
  if (n.includes("risk") || n.includes("analyst")) return BarChart3;
  if (n.includes("developer") || n.includes("devel") || n.includes("backend") || n.includes("frontend") || n.includes("engineer")) return Code2;
  if (n.includes("em ") || n.includes("manager") || n.includes("director")) return UserCog;
  if (n.includes("lp ") || n.includes("contract") || n.includes("defi") || n.includes("bankroll") || n.includes("liquidity")) return Lock;
  if (n.includes("growth") || n.includes("revenue") || n.includes("sales")) return TrendingUp;
  return Bot;
}

export function WorkspaceSidebar({
  workspaceLabel,
  workspaceInitial,
  companies,
  selectedCompanyId,
  onSelectCompany,
  view,
  onNavigate,
  dashboardLiveCount,
  inboxCount,
  companyWorkTab = null,
  onNavigateInbox,
  onNavigateTasks,
  projects = [],
  selectedProjectId = null,
  onSelectProject,
  goals,
  agents,
  selectedAgentPersona = null,
  onSelectAgent,
  onDeleteRegistryAgent,
  onAddRegistryAgent,
  onNewIssue,
  onOpenOnboarding,
  onDeleteCompany,
  apiBase,
  catalog,
  onCreateFromCatalog,
}: WorkspaceSidebarProps) {
  const [coOpen, setCoOpen] = useState(false);
  const [workspaceSearch, setWorkspaceSearch] = useState("");
  /** `owner/repo/slug` while install + import runs for that pack */
  const [catalogCreatingPath, setCatalogCreatingPath] = useState<string | null>(null);
  const [deletingCompanyId, setDeletingCompanyId] = useState<string | null>(null);
  const [projectsOpen, setProjectsOpen] = useState(true);
  const [goalsOpen, setGoalsOpen] = useState(true);
  const [agOpen, setAgOpen] = useState(true);
  const [devOpen, setDevOpen] = useState(false);
  /** Directory list: everything on companies.sh vs Paperclip agent-company packs only */
  const [directoryScope, setDirectoryScope] = useState<"all" | "paperclip">("all");

  const sortedAgents = useMemo(
    () => [...agents].sort((a, b) => b.liveCount - a.liveCount || a.name.localeCompare(b.name)),
    [agents]
  );

  const q = workspaceSearch.trim().toLowerCase();
  const filteredLocal = useMemo(() => {
    if (!q) return companies;
    return companies.filter(
      (c) => c.display_name.toLowerCase().includes(q) || c.id.toLowerCase().includes(q)
    );
  }, [companies, q]);

  const filteredCatalog = useMemo(() => {
    const raw = catalog?.items ?? [];
    const scoped = directoryScope === "paperclip" ? raw.filter(isPaperclipPack) : raw;
    if (!q) return scoped;
    return scoped.filter((it) => {
      const hay = `${it.name} ${it.slug} ${it.repo} ${it.tagline ?? ""} ${it.category ?? ""}`.toLowerCase();
      return hay.includes(q);
    });
  }, [catalog?.items, q, directoryScope]);

  const paperclipPackCount = useMemo(
    () => (catalog?.items ?? []).filter(isPaperclipPack).length,
    [catalog?.items]
  );

  return (
    <aside className="flex h-screen w-[272px] shrink-0 flex-col border-r border-[#222222] bg-[#000000]">
      <div className="border-b border-[#222222] p-3">
        <div className="relative">
          <button
            type="button"
            className="flex w-full items-center gap-2.5 rounded-lg p-1.5 text-left transition-colors duration-200 ease-out hover:bg-[#111111]"
            onClick={() => setCoOpen((o) => !o)}
          >
            <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-[#1A1A1A] text-sm font-semibold text-white">
              {workspaceInitial.slice(0, 1).toUpperCase()}
            </span>
            <div className="min-w-0 flex-1">
              <div className="truncate text-sm font-medium text-white">{workspaceLabel}</div>
              <div className="font-mono text-[11px] uppercase tracking-wide text-[#666666]">Company OS</div>
            </div>
            <ChevronDown
              className={cn("h-4 w-4 shrink-0 text-[#666666] transition-transform duration-200 ease-out", coOpen && "rotate-180")}
            />
          </button>
          {coOpen ? (
            <div className="absolute left-0 right-0 top-full z-20 mt-1 max-h-[min(70vh,440px)] overflow-hidden rounded-lg border border-[#333333] bg-[#111111]">
              <div className="border-b border-[#222222] p-2">
                <input
                  type="search"
                  value={workspaceSearch}
                  onChange={(e) => setWorkspaceSearch(e.target.value)}
                  placeholder="Search workspaces & directory…"
                  className="w-full rounded-md border border-[#333333] bg-[#000000] px-2 py-1.5 font-mono text-xs text-[#E8E8E8] outline-none placeholder:text-[#666666] focus:border-[#5B9BF6]"
                  autoFocus
                />
              </div>
              <div className="max-h-[min(60vh,380px)] overflow-y-auto">
                {filteredLocal.length > 0 ? (
                  <>
                    <div className="px-2 py-1.5">
                      <div className="font-mono text-[10px] font-semibold uppercase tracking-[0.1em] text-[#666666]">
                        Your companies
                      </div>
                      <p className="mt-1 font-mono text-[9px] uppercase leading-snug tracking-[0.06em] text-[#555555]">
                        Row = switch · Slug = id · Trash = del db only
                      </p>
                    </div>
                    <ul className="pb-2">
                      {filteredLocal.map((c) => (
                        <li key={c.id} className="flex items-stretch border-b border-[#1a1a1a] last:border-b-0">
                          <button
                            type="button"
                            className={cn(
                              "min-w-0 flex-1 px-3 py-2 text-left text-sm transition-colors duration-200 ease-out",
                              selectedCompanyId === c.id
                                ? "border-l-2 border-[#5B9BF6] bg-[#1A1A1A] text-[#E8E8E8]"
                                : "text-[#999999] hover:bg-[#1A1A1A] hover:text-[#E8E8E8]"
                            )}
                            onClick={() => {
                              onSelectCompany(c.id);
                              setCoOpen(false);
                              setWorkspaceSearch("");
                            }}
                          >
                            <span className="block truncate font-sans text-[13px] font-medium leading-tight">
                              {c.display_name}
                            </span>
                            <span className="mt-1 flex min-w-0 items-baseline gap-1 truncate">
                              <span className="shrink-0 font-mono text-[8px] font-semibold uppercase tracking-[0.12em] text-[#666666]">
                                Id
                              </span>
                              <span className="min-w-0 truncate font-mono text-[9px] uppercase tracking-[0.06em] text-[#888888]">
                                {c.slug}
                              </span>
                            </span>
                          </button>
                          {onDeleteCompany ? (
                            <button
                              type="button"
                              title={`Remove from database: ${c.display_name} (${c.slug})`}
                              aria-label={`Delete workspace ${c.display_name}`}
                              disabled={deletingCompanyId === c.id}
                              className={cn(
                                "flex w-11 shrink-0 flex-col items-center justify-center gap-0.5 text-[#666666] transition-colors duration-200 ease-out hover:bg-[#2a1212] hover:text-[#D71921]",
                                deletingCompanyId === c.id && "cursor-wait opacity-50"
                              )}
                              onClick={async (e) => {
                                e.preventDefault();
                                e.stopPropagation();
                                setDeletingCompanyId(c.id);
                                try {
                                  await onDeleteCompany({
                                    id: c.id,
                                    slug: c.slug,
                                    display_name: c.display_name,
                                  });
                                } finally {
                                  setDeletingCompanyId(null);
                                }
                              }}
                            >
                              {deletingCompanyId === c.id ? (
                                <Loader2 className="h-3.5 w-3.5 animate-spin stroke-[1.5]" aria-hidden />
                              ) : (
                                <Trash2 className="h-3.5 w-3.5 stroke-[1.5]" aria-hidden />
                              )}
                              <span className="font-mono text-[7px] font-semibold uppercase tracking-[0.14em]">
                                {deletingCompanyId === c.id ? "…" : "Del"}
                              </span>
                            </button>
                          ) : null}
                        </li>
                      ))}
                    </ul>
                  </>
                ) : companies.length === 0 && !q ? (
                  <p className="px-3 py-2 font-mono text-xs text-[#666666]">No local companies yet.</p>
                ) : companies.length > 0 && filteredLocal.length === 0 ? (
                  <p className="px-3 py-2 font-mono text-xs text-[#666666]">No local match.</p>
                ) : null}

                {catalog ? (
                  <>
                    <div className="border-t border-[#222222] px-2 py-1.5">
                      <div className="font-mono text-[10px] font-semibold uppercase tracking-[0.1em] text-[#666666]">
                        companies.sh —{" "}
                        <a
                          href="https://companies.sh/"
                          target="_blank"
                          rel="noreferrer"
                          className="text-[#5B9BF6] hover:text-white"
                          onClick={(e) => e.stopPropagation()}
                        >
                          open directory
                        </a>
                        <span className="mt-1 block font-normal normal-case tracking-normal text-[#484848]">
                          With{" "}
                          <code className="text-[#666666]">HSM_COMPANY_PACK_INSTALL_ROOT</code> on the Next server, picking
                          a pack runs <code className="text-[#666666]">npx companies.sh add owner/repo/slug</code> there
                          and links <code className="text-[#666666]">hsmii_home</code>.
                        </span>
                      </div>
                      <div className="mt-2 flex flex-wrap gap-1">
                        <button
                          type="button"
                          title="All published agent companies"
                          onClick={(e) => {
                            e.stopPropagation();
                            setDirectoryScope("all");
                          }}
                          className={cn(
                            "rounded-full px-2 py-0.5 font-mono text-[9px] font-semibold uppercase tracking-wide transition-colors",
                            directoryScope === "all"
                              ? "bg-white text-black"
                              : "border border-[#333333] text-[#888888] hover:border-[#555555] hover:text-[#CCCCCC]"
                          )}
                        >
                          All packs
                        </button>
                        <button
                          type="button"
                          title="Templates from paperclipai/companies (Paperclip roster)"
                          onClick={(e) => {
                            e.stopPropagation();
                            setDirectoryScope("paperclip");
                          }}
                          className={cn(
                            "rounded-full px-2 py-0.5 font-mono text-[9px] font-semibold uppercase tracking-wide transition-colors",
                            directoryScope === "paperclip"
                              ? "bg-[#58a6ff]/20 text-[#58a6ff] ring-1 ring-[#58a6ff]/40"
                              : "border border-[#333333] text-[#888888] hover:border-[#555555] hover:text-[#CCCCCC]"
                          )}
                        >
                          Paperclip ({catalog.loading ? "…" : paperclipPackCount})
                        </button>
                        <a
                          href="https://github.com/paperclipai/companies"
                          target="_blank"
                          rel="noreferrer"
                          title="Upstream: 16 companies, 440+ agents"
                          className="ml-auto self-center font-mono text-[9px] text-[#5B9BF6] hover:text-white"
                          onClick={(e) => e.stopPropagation()}
                        >
                          paperclipai/companies →
                        </a>
                      </div>
                      {directoryScope === "paperclip" ? (
                        <p className="mt-2 rounded-md border border-[#58a6ff]/30 bg-[#58a6ff]/10 px-2.5 py-2 font-mono text-[10px] leading-snug text-[#9ecbff]">
                          <strong className="font-semibold text-[#c8e1ff]">Important:</strong> each template run installs
                          files (when <code className="text-[#79b8ff]">HSM_COMPANY_PACK_INSTALL_ROOT</code> is set) and{" "}
                          <strong className="text-[#c8e1ff]">imports every agent</strong> into Team &amp; setup and{" "}
                          <strong className="text-[#c8e1ff]">indexes pack skills</strong> into company context. Keep the
                          menu open until the row stops spinning.
                        </p>
                      ) : null}
                    </div>
                    {catalog.loading ? (
                      <p className="px-3 py-2 font-mono text-xs text-[#666666]">[LOADING DIRECTORY…]</p>
                    ) : catalog.error ? (
                      <p className="px-3 py-2 font-mono text-xs text-[#D71921]">{catalog.error}</p>
                    ) : (
                      <ul className="pb-2">
                        {filteredCatalog.slice(0, 80).map((it) => {
                          const path = companiesShInstallPath(it);
                          const busy = catalogCreatingPath === path;
                          const packBase = slugBaseFromCatalogItem(it);
                          const existingWs =
                            findExistingCompanyForCatalogPack(companies, packBase) ??
                            findCompanyByPackFolder(companies, it.slug);
                          return (
                            <li key={`${it.repo}/${it.slug}`}>
                              <button
                                type="button"
                                disabled={!onCreateFromCatalog || busy}
                                className={cn(
                                  "w-full px-3 py-2 text-left transition-colors duration-200 ease-out disabled:cursor-not-allowed disabled:opacity-50",
                                  "hover:bg-[#1A1A1A]"
                                )}
                                onClick={async () => {
                                  if (!onCreateFromCatalog) return;
                                  setCatalogCreatingPath(path);
                                  try {
                                    await onCreateFromCatalog(it);
                                    setCoOpen(false);
                                    setWorkspaceSearch("");
                                  } finally {
                                    setCatalogCreatingPath(null);
                                  }
                                }}
                              >
                                <div className="flex flex-wrap items-center gap-1.5">
                                  {busy ? (
                                    <Loader2
                                      className="h-3.5 w-3.5 shrink-0 animate-spin text-[#58a6ff]"
                                      aria-hidden
                                    />
                                  ) : null}
                                  <span className="text-sm text-[#E8E8E8]">{it.name}</span>
                                  {isPaperclipPack(it) ? (
                                    <span className="rounded border border-[#58a6ff]/35 bg-[#58a6ff]/10 px-1.5 py-px font-mono text-[9px] font-semibold uppercase tracking-wide text-[#58a6ff]">
                                      Paperclip
                                    </span>
                                  ) : null}
                                  {existingWs ? (
                                    <span className="rounded border border-[#333333] bg-[#1A1A1A] px-1.5 py-px font-mono text-[9px] font-medium uppercase tracking-wide text-[#888888]">
                                      Open workspace
                                    </span>
                                  ) : (
                                    <span className="rounded border border-[#333333] px-1.5 py-px font-mono text-[9px] font-medium uppercase tracking-wide text-[#666666]">
                                      Add
                                    </span>
                                  )}
                                </div>
                                <div className="mt-0.5 font-mono text-[10px] uppercase tracking-wide text-[#666666]">
                                  {path}
                                  {it.installs ? ` · ${it.installs} installs` : ""}
                                  {it.githubStars ? ` · ★ ${it.githubStars}` : ""}
                                </div>
                              </button>
                            </li>
                          );
                        })}
                        {filteredCatalog.length > 80 ? (
                          <li className="px-3 py-2 font-mono text-[10px] text-[#666666]">
                            Refine search — {filteredCatalog.length} matches.
                          </li>
                        ) : null}
                        {filteredCatalog.length === 0 && !catalog.loading ? (
                          <li className="px-3 py-2 font-mono text-xs text-[#666666]">No directory match.</li>
                        ) : null}
                      </ul>
                    )}
                  </>
                ) : null}
              </div>
            </div>
          ) : null}
        </div>

        {companies.length > 1 ? (
          <div className="mt-3 border-t border-[#222222] pt-2">
            <p className="mb-1 px-0.5 font-mono text-[10px] font-semibold uppercase tracking-[0.1em] text-[#666666]">
              Your companies
            </p>
            <ul className="max-h-[min(7.5rem,28vh)] space-y-0.5 overflow-y-auto pr-0.5">
              {companies.map((c) => {
                const selected = selectedCompanyId === c.id;
                return (
                  <li key={c.id}>
                    <button
                      type="button"
                      title={`Switch to ${c.display_name}`}
                      onClick={() => {
                        onSelectCompany(c.id);
                        setCoOpen(false);
                      }}
                      className={cn(
                        "w-full truncate rounded py-1.5 pl-2 pr-2 text-left text-sm transition-colors duration-200 ease-out",
                        selected
                          ? "border-l-2 border-[#D71921] bg-[#1A1A1A] pl-[6px] text-white"
                          : "text-[#999999] hover:bg-[#111111] hover:text-[#E8E8E8]"
                      )}
                    >
                      {c.display_name}
                    </button>
                  </li>
                );
              })}
            </ul>
          </div>
        ) : null}

        <button
          type="button"
          onClick={onNewIssue}
          className="mt-3 flex h-11 w-full items-center justify-center gap-2 rounded-full border border-[#333333] bg-transparent py-2 font-mono text-[13px] font-normal uppercase tracking-[0.06em] text-[#E8E8E8] transition-colors duration-200 ease-out hover:border-[#E8E8E8] hover:text-white"
        >
          <Plus className="h-4 w-4" strokeWidth={1.5} />
          NEW TASK
        </button>
      </div>

      <nav className="flex-1 overflow-y-auto px-2 py-3">
        <div className="space-y-1 ">
          <NavButton
            active={view === "command"}
            onClick={() => onNavigate("command")}
            icon={LayoutDashboard}
            label="Dashboard"
            badge={dashboardLiveCount > 0 ? dashboardLiveCount : undefined}
            badgeVariant="live"
          />
          {onNavigateInbox ? (
            <NavButton
              active={view === "company" && companyWorkTab === "inbox"}
              onClick={onNavigateInbox}
              icon={Inbox}
              label="Inbox"
              title="Human decisions only — agents surface work when they need you"
              badge={inboxCount > 0 ? inboxCount : undefined}
            />
          ) : null}
          {onNavigateTasks ? (
            <NavButton
              active={view === "company" && companyWorkTab === "tasks"}
              onClick={onNavigateTasks}
              icon={LayoutList}
              label="Tasks"
              title="Full task graph — checkout, SLA, filters, backlog"
            />
          ) : (
            <NavButton
              active={view === "company"}
              onClick={() => onNavigate("company")}
              icon={Inbox}
              label="Company"
              title="Workspace"
            />
          )}
          <NavButton
            active={view === "marketplace"}
            onClick={() => onNavigate("marketplace")}
            icon={Store}
            label="Marketplace"
            title="Browse agent-company templates and add workspaces"
          />
          <NavButton
            active={view === "sops"}
            onClick={() => onNavigate("sops")}
            icon={BookOpen}
            label="Playbooks"
            title="Author SOPs and playbooks, then implement them as tasks"
          />
        </div>

        <button
          type="button"
          className="mb-1 mt-4 flex w-full items-center justify-between px-2 py-1 font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-[#666666]"
          onClick={() => setProjectsOpen((o) => !o)}
        >
          <span>Projects</span>
          <ChevronDown className={cn("h-3.5 w-3.5 transition-transform", projectsOpen && "rotate-180")} />
        </button>
        {projectsOpen ? (
          <div className="space-y-0.5">
            {projects.length === 0 ? (
              <p className="px-2 py-2 font-mono text-[11px] leading-snug text-[#666666]">
                None yet. Open <span className="text-[#8B949E]">Tasks</span> to add a project, then attach new issues to it.
              </p>
            ) : (
              projects.slice(0, 16).map((p) => {
                const active = selectedProjectId === p.id;
                return (
                  <button
                    key={p.id}
                    type="button"
                    onClick={() => {
                      onSelectProject?.(p.id);
                      onNavigateTasks?.();
                    }}
                    className={cn(
                      "flex w-full items-center gap-2 rounded-md px-2 py-1.5 pl-3 text-left text-[13px] transition-colors duration-200 ease-out",
                      active
                        ? "bg-white text-black"
                        : "text-[#999999] hover:bg-[#111111] hover:text-[#E8E8E8]"
                    )}
                  >
                    <Folder
                      className={cn("h-3.5 w-3.5 shrink-0 stroke-[1.5]", active ? "text-black" : "text-[#666666]")}
                    />
                    <span className="truncate">{p.title}</span>
                  </button>
                );
              })
            )}
          </div>
        ) : null}

        <button
          type="button"
          className="mb-1 mt-4 flex w-full items-center justify-between px-2 py-1 font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-[#666666]"
          onClick={() => setGoalsOpen((o) => !o)}
        >
          <span>Goals</span>
          <ChevronDown className={cn("h-3.5 w-3.5 transition-transform", goalsOpen && "rotate-180")} />
        </button>
        {goalsOpen ? (
          <div className="space-y-0.5">
            {goals.length === 0 ? (
              <p className="px-2 py-2 font-mono text-[11px] leading-snug text-[#666666]">
                None for this workspace. Open <span className="text-[#8B949E]">Advanced</span> →{" "}
                <span className="text-[#8B949E]">Goals &amp; governance log</span> to add one.
              </p>
            ) : (
              goals.slice(0, 12).map((g, i) => (
                <button
                  key={g.id}
                  type="button"
                  onClick={() => (onNavigateTasks ? onNavigateTasks() : onNavigate("company"))}
                  className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 pl-3 text-left text-[13px] text-[#999999] transition-colors duration-200 ease-out hover:bg-[#111111] hover:text-[#E8E8E8]"
                >
                  <span
                    className="h-2.5 w-2.5 shrink-0 rounded-full"
                    style={{ backgroundColor: GOAL_ROW_DOT_COLORS[i % GOAL_ROW_DOT_COLORS.length] }}
                  />
                  <span className="truncate">{g.title}</span>
                </button>
              ))
            )}
          </div>
        ) : null}

        <div className="mb-1 mt-4 flex items-center justify-between gap-1 px-2">
          <button
            type="button"
            className="flex min-w-0 flex-1 items-center justify-between py-1 font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-[#666666]"
            onClick={() => setAgOpen((o) => !o)}
          >
            <span className="flex items-center gap-1.5">
              <Bot className="h-3.5 w-3.5" strokeWidth={1.5} />
              Agents
            </span>
            <ChevronDown className={cn("h-3.5 w-3.5 shrink-0 transition-transform", agOpen && "rotate-180")} />
          </button>
          {onAddRegistryAgent ? (
            <button
              type="button"
              title="Add workforce agent (opens Team & setup)"
              onClick={(e) => {
                e.stopPropagation();
                onAddRegistryAgent();
              }}
              className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-[#333333] text-[#999999] transition-colors hover:border-[#5B9BF6] hover:text-[#E8E8E8]"
            >
              <Plus className="h-3.5 w-3.5" strokeWidth={1.5} />
            </button>
          ) : null}
        </div>
        {agOpen ? (
          <div className="max-h-56 space-y-0.5 overflow-y-auto">
            {sortedAgents.length === 0 ? (
              <p className="px-2 py-2 font-mono text-xs text-[#666666]">Agents appear from task owners & checkouts.</p>
            ) : (
              <>
                {sortedAgents.map((a) => {
                const active = selectedAgentPersona === a.id;
                const AgentIcon = getAgentIcon(a.name);
                const rowTone = cn(
                  "flex w-full items-center gap-2 rounded-md px-2 py-1.5 pl-3 text-left text-[13px] transition-colors duration-200 ease-out",
                  active
                    ? "bg-white text-black"
                    : "text-[#999999] hover:bg-[#111111] hover:text-[#E8E8E8]"
                );
                const label = (
                  <>
                    <AgentIcon
                      className={cn("h-3.5 w-3.5 shrink-0 stroke-[1.5]", active ? "text-black" : "text-[#666666]")}
                    />
                    <span className="min-w-0 flex-1 truncate text-[13px]">{a.name}</span>
                    {a.liveCount > 0 ? (
                      <span
                        className={cn(
                          "shrink-0 rounded border px-1.5 py-0.5 font-mono text-[10px] font-semibold",
                          active
                            ? "border-[#333333] bg-[#F0F0F0] text-[#1A1A1A]"
                            : "border-[#4A9E5C]/50 bg-[#4A9E5C]/15 text-[#4A9E5C]"
                        )}
                      >
                        {a.liveCount} live
                      </span>
                    ) : null}
                  </>
                );
                if (a.registryAgentId && onDeleteRegistryAgent) {
                  return (
                    <div
                      key={a.id}
                      className={cn(
                        "flex w-full items-stretch overflow-hidden rounded-md",
                        active ? "bg-white text-black" : "text-[#999999] hover:bg-[#111111] hover:text-[#E8E8E8]"
                      )}
                    >
                      <button
                        type="button"
                        title={`View tasks for ${a.name}`}
                        onClick={() => onSelectAgent?.(a.id)}
                        className={cn(
                          "flex min-w-0 flex-1 items-center gap-2 py-1.5 pl-3 pr-1 text-left text-[13px]",
                          active ? "" : "rounded-l-md"
                        )}
                      >
                        {label}
                      </button>
                      <button
                        type="button"
                        title="Remove from workforce roster"
                        onClick={(e) => {
                          e.stopPropagation();
                          onDeleteRegistryAgent(a.registryAgentId!, a.id);
                        }}
                        className={cn(
                          "flex w-7 shrink-0 items-center justify-center rounded-r-md border-l transition-colors",
                          active
                            ? "border-[#DDDDDD] text-[#333333] hover:bg-[#EAEAEA]"
                            : "border-[#222222] text-[#666666] hover:bg-[#1A1A1A] hover:text-[#D71921]"
                        )}
                      >
                        <Trash2 className="h-3 w-3" strokeWidth={1.5} />
                      </button>
                    </div>
                  );
                }
                return (
                  <button
                    key={a.id}
                    type="button"
                    title={`View tasks for ${a.name}`}
                    onClick={() => onSelectAgent?.(a.id)}
                    className={rowTone}
                  >
                    {label}
                  </button>
                );
              })}
              </>
            )}
          </div>
        ) : null}

        <SectionTitle>Work</SectionTitle>
        <div className="space-y-0.5">
          {!onNavigateInbox && !onNavigateTasks ? (
            <SubLink active={view === "company"} onClick={() => onNavigate("company")} label="Company" />
          ) : null}
          <SubLink
            active={view === "quality"}
            onClick={() => onNavigate("quality")}
            label="Routines"
            suffix={
              <span className="rounded border border-[#D4A843]/50 px-1 font-mono text-[9px] font-semibold uppercase tracking-wide text-[#D4A843]">
                Beta
              </span>
            }
          />
        </div>

        <div className="mt-8 border-t border-[#222222] pt-3">
          <button
            type="button"
            className="flex w-full items-center justify-between px-2 py-1 font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-[#666666]"
            onClick={() => setDevOpen((o) => !o)}
          >
            <span>HSM tools</span>
            <ChevronDown className={cn("h-3.5 w-3.5 transition-transform", devOpen && "rotate-180")} />
          </button>
          {devOpen ? (
            <div className="mt-1 space-y-0.5">
              <SubLink active={view === "dash"} onClick={() => onNavigate("dash")} label="System overview" />
              <SubLink active={view === "quality"} onClick={() => onNavigate("quality")} label="Quality loop" />
              <SubLink active={view === "onboard"} onClick={() => onNavigate("onboard")} label="Onboarding" />
              <SubLink active={view === "email"} onClick={() => onNavigate("email")} label="Email draft" />
              <SubLink active={view === "trail"} onClick={() => onNavigate("trail")} label="Trail" />
              <SubLink active={view === "memory"} onClick={() => onNavigate("memory")} label="Memory" />
              <SubLink active={view === "graph"} onClick={() => onNavigate("graph")} label="Graph" />
              <SubLink active={view === "search"} onClick={() => onNavigate("search")} label="Search" />
            </div>
          ) : null}
        </div>

        {onOpenOnboarding ? (
          <button
            type="button"
            onClick={onOpenOnboarding}
            className="mt-4 flex w-full items-center gap-2 rounded-lg border border-dashed border-[#333333] px-2.5 py-2 text-left text-sm text-[#999999] transition-colors duration-200 ease-out hover:border-[#5B9BF6] hover:text-[#E8E8E8]"
          >
            <Sparkles className="h-4 w-4 shrink-0" strokeWidth={1.5} />
            Guided setup
          </button>
        ) : null}
      </nav>

      <div className="border-t border-[#222222] p-3 font-mono text-[10px] uppercase tracking-wide text-[#666666]">
        <code className="break-all normal-case tracking-normal text-[#666666]">{apiBase}</code>
      </div>
    </aside>
  );
}
