"use client";

import Link from "next/link";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import {
  Fragment,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import {
  BookOpen,
  Bot,
  BrainCircuit,
  CircleDollarSign,
  ClipboardList,
  Command as CommandIcon,
  FolderKanban,
  LayoutDashboard,
  Layers,
  Network,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRight,
  PanelRightClose,
  ShieldCheck,
  Store,
  type LucideIcon,
} from "lucide-react";
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from "@/app/components/ui/breadcrumb";
import { Button } from "@/app/components/ui/button";
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
} from "@/app/components/ui/command";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/app/components/ui/select";
import { Separator } from "@/app/components/ui/separator";
import { cn } from "@/app/lib/utils";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { WorkspaceRightRail } from "@/app/components/console/WorkspaceRightRail";
import { WorkspaceProjectsNav } from "@/app/components/console/WorkspaceProjectsNav";
import { WorkspaceHubLinks } from "@/app/components/workspace/WorkspaceHubLinks";

type NavItem = { href: string; label: string; icon: LucideIcon; title: string };

const NAV_SECTIONS: { heading: string; items: NavItem[] }[] = [
  {
    heading: "Work",
    items: [
      { href: "/workspace/dashboard", label: "Dashboard", icon: LayoutDashboard, title: "Overview and queues" },
      { href: "/workspace/issues", label: "Issues", icon: FolderKanban, title: "Tasks and filters" },
      { href: "/workspace/approvals", label: "Approvals", icon: ShieldCheck, title: "Human and policy gates" },
      { href: "/workspace/my-work", label: "My work", icon: ClipboardList, title: "Your queue" },
    ],
  },
  {
    heading: "Team & procedures",
    items: [
      { href: "/workspace/agents", label: "Agents", icon: Bot, title: "Roster, files, memory, skills" },
      { href: "/workspace/playbooks", label: "Playbooks", icon: BookOpen, title: "SOPs and templates" },
      { href: "/workspace/marketplace", label: "Marketplace", icon: Store, title: "Install company packs" },
    ],
  },
  {
    heading: "Insights",
    items: [
      { href: "/workspace/intelligence", label: "Intelligence", icon: BrainCircuit, title: "Goals, feed, summary" },
      { href: "/workspace/costs", label: "Costs", icon: CircleDollarSign, title: "Spend" },
      { href: "/workspace/graph", label: "Graph", icon: Network, title: "Hypergraph views" },
      { href: "/workspace/architecture", label: "Architecture", icon: Layers, title: "System blueprint" },
    ],
  },
];

type Crumb = { label: string; href: string | null };

const SIDEBAR_OPEN_KEY = "pc-ws-sidebar-open";

const AGENT_TAB_LABELS: Record<string, string> = {
  workspace: "Workspace",
  memory: "Memory",
  instructions: "Instructions",
  dashboard: "Dashboard",
  chat: "Chat",
  configuration: "Configuration",
  runs: "Runs",
};

function humanizePackName(name: string): string {
  return name
    .replace(/_/g, " ")
    .split(/\s+/)
    .filter(Boolean)
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1).toLowerCase())
    .join(" ");
}

function breadcrumbsForPath(pathname: string): Crumb[] {
  const root: Crumb = { label: "Workspace", href: "/workspace/dashboard" };
  if (pathname.startsWith("/workspace/dashboard")) return [root, { label: "Dashboard", href: null }];
  if (/^\/workspace\/agents\/[0-9a-fA-F-]{36}\/?$/.test(pathname)) {
    return [
      root,
      { label: "Agents", href: "/workspace/agents" },
      { label: "Agent", href: null },
    ];
  }
  if (pathname.startsWith("/workspace/agents")) return [root, { label: "Agents", href: null }];
  if (pathname.startsWith("/workspace/issues")) return [root, { label: "Issues", href: null }];
  if (pathname.startsWith("/workspace/approvals")) return [root, { label: "Approvals", href: null }];
  if (pathname.startsWith("/workspace/costs")) return [root, { label: "Costs", href: null }];
  if (pathname.startsWith("/workspace/graph")) return [root, { label: "Graph", href: null }];
  if (pathname.startsWith("/workspace/architecture")) return [root, { label: "Architecture", href: null }];
  if (pathname.startsWith("/workspace/intelligence")) return [root, { label: "Intelligence", href: null }];
  if (pathname.startsWith("/workspace/marketplace")) return [root, { label: "Marketplace", href: null }];
  if (pathname.startsWith("/workspace/playbooks")) return [root, { label: "Playbooks", href: null }];
  if (pathname.startsWith("/workspace/my-work")) return [root, { label: "My Work", href: null }];
  return [root];
}

function BreadcrumbTrail({ crumbs }: { crumbs: Crumb[] }) {
  return (
    <BreadcrumbList className="flex-nowrap font-mono text-[11px] uppercase tracking-[0.06em] text-[#999999]">
      {crumbs.map((c, i) => (
        <span key={`${c.label}-${i}`} className="contents">
          {i > 0 ? <BreadcrumbSeparator /> : null}
          <BreadcrumbItem>
            {c.href ? (
              <BreadcrumbLink asChild>
                <Link href={c.href} className="text-[#999999] hover:text-[#e8e8e8]">
                  {c.label}
                </Link>
              </BreadcrumbLink>
            ) : (
              <BreadcrumbPage className="font-medium text-[#e8e8e8]">{c.label}</BreadcrumbPage>
            )}
          </BreadcrumbItem>
        </span>
      ))}
    </BreadcrumbList>
  );
}

/**
 * Syncs `?tab=` into parent state after mount. Kept in a nested Suspense so the shell never
 * SSR/hydrates different breadcrumb text than the first client paint (useSearchParams alone
 * can diverge from the Suspense fallback and trigger hydration warnings).
 */
function AgentTabLabelSync({ setTabLabel }: { setTabLabel: Dispatch<SetStateAction<string>> }) {
  const searchParams = useSearchParams();
  useEffect(() => {
    const tab = searchParams.get("tab") ?? "workspace";
    setTabLabel(AGENT_TAB_LABELS[tab] ?? AGENT_TAB_LABELS.workspace);
  }, [searchParams, setTabLabel]);
  return null;
}

function AgentDetailBreadcrumbs() {
  const pathname = usePathname() ?? "";
  const { propertiesSelection } = useWorkspace();
  /** Stable first paint: matches server and avoids hydration mismatch; AgentTabLabelSync updates from the URL. */
  const [tabLabel, setTabLabel] = useState("Workspace");

  const crumbs = useMemo((): Crumb[] => {
    const root: Crumb = { label: "Workspace", href: "/workspace/dashboard" };
    const agentMatch = pathname.match(/^\/workspace\/agents\/([0-9a-fA-F-]{36})\/?$/i);
    if (!agentMatch) {
      return breadcrumbsForPath(pathname);
    }
    const id = agentMatch[1];
    const rawName =
      propertiesSelection?.kind === "agent" && propertiesSelection.id === id
        ? propertiesSelection.name ?? ""
        : "";
    const displayName = rawName ? humanizePackName(rawName) : "Agent";
    return [
      root,
      { label: "Agents", href: "/workspace/agents" },
      { label: displayName, href: null },
      { label: tabLabel, href: null },
    ];
  }, [pathname, propertiesSelection, tabLabel]);

  return (
    <div className="flex min-w-0 flex-1 items-center">
      <Suspense fallback={null}>
        <AgentTabLabelSync setTabLabel={setTabLabel} />
      </Suspense>
      <Breadcrumb className="min-w-0 flex-1">
        <BreadcrumbTrail crumbs={crumbs} />
      </Breadcrumb>
    </div>
  );
}

export function ConsoleAppShell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const router = useRouter();
  const {
    apiBase,
    companyId,
    setCompanyId,
    companies,
    companiesLoading,
    postgresOk,
    apiHealthError,
    postgresConfigured,
    propertiesSelection,
  } = useWorkspace();

  const [cmdOpen, setCmdOpen] = useState(false);
  const [propsOpen, setPropsOpen] = useState(true);
  const [sidebarOpen, setSidebarOpen] = useState(true);

  useEffect(() => {
    try {
      const v = window.localStorage.getItem(SIDEBAR_OPEN_KEY);
      if (v === "0") setSidebarOpen(false);
    } catch {
      /* ignore */
    }
  }, []);

  useEffect(() => {
    try {
      window.localStorage.setItem(SIDEBAR_OPEN_KEY, sidebarOpen ? "1" : "0");
    } catch {
      /* ignore */
    }
  }, [sidebarOpen]);

  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (e.key === "k" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        setCmdOpen((o) => !o);
      }
    };
    window.addEventListener("keydown", down);
    return () => window.removeEventListener("keydown", down);
  }, []);

  useEffect(() => {
    if (propertiesSelection) setPropsOpen(true);
  }, [propertiesSelection]);

  const defaultCrumbs = breadcrumbsForPath(pathname ?? "");
  const onAgentDetail = /^\/workspace\/agents\/[0-9a-fA-F-]{36}\/?$/i.test(pathname ?? "");

  const runCommand = useCallback(
    (fn: () => void) => {
      setCmdOpen(false);
      fn();
    },
    [],
  );

  return (
    <div
      className="flex h-[100dvh] max-h-[100dvh] min-h-0 w-full flex-row overflow-hidden bg-black text-foreground"
      suppressHydrationWarning
    >
      {sidebarOpen ? (
        <aside className="flex h-full min-h-0 w-[15.5rem] shrink-0 flex-col overflow-hidden border-r border-[#222222] bg-[#111111]">
          <div className="shrink-0 border-b border-[#222222] px-3 py-3">
            <div className="flex items-start gap-1">
              <Link
                href="/workspace/dashboard"
                className="min-w-0 flex-1 outline-none ring-offset-black focus-visible:ring-2 focus-visible:ring-[#333333]"
              >
                <span className="pc-sidebar-brand">Company console</span>
                <span className="pc-sidebar-eyebrow">Workspace</span>
              </Link>
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                className="shrink-0 border border-transparent text-[#888888] hover:bg-white/[0.06] hover:text-[#e8e8e8]"
                title="Collapse workspace navigation"
                onClick={() => setSidebarOpen(false)}
              >
                <PanelLeftClose className="size-4" strokeWidth={1.5} aria-hidden />
                <span className="sr-only">Collapse workspace navigation</span>
              </Button>
            </div>
          </div>
          <nav className="flex min-h-0 flex-1 flex-col gap-px overflow-y-auto overscroll-contain p-2" aria-label="Workspace">
            {NAV_SECTIONS.map((section) => (
              <Fragment key={section.heading}>
                <div className="nd-ws-nav-section-label">{section.heading}</div>
                {section.items.map(({ href, label, icon: Icon, title }) => {
                  const active =
                    pathname === href ||
                    (href !== "/workspace/dashboard" && pathname?.startsWith(href)) ||
                    (href === "/workspace/agents" && /^\/workspace\/agents\//i.test(pathname ?? ""));
                  return (
                    <Link
                      key={href}
                      href={href}
                      title={title}
                      className={cn("nd-ws-nav-link", active ? "nd-ws-nav-link--active" : "nd-ws-nav-link--idle")}
                    >
                      <Icon className="size-4 shrink-0 opacity-90" strokeWidth={1.5} aria-hidden />
                      {label}
                    </Link>
                  );
                })}
              </Fragment>
            ))}
            <div className="mt-1 border-t border-[#222222] pt-2">
              <WorkspaceProjectsNav />
            </div>
            <Separator className="my-2 bg-[#222222]" />
            <Link
              href="/"
              className="nd-ws-nav-link nd-ws-nav-link--idle rounded-sm px-2 py-2 font-mono text-[11px] uppercase tracking-[0.06em]"
            >
              Legacy console →
            </Link>
          </nav>
          <div className="shrink-0 border-t border-[#222222] p-2 font-mono text-[10px] uppercase tracking-[0.06em] text-[#666666]">
            API
            <div className="mt-0.5 break-all font-mono text-[9px] normal-case tracking-normal text-[#999999]">{apiBase || "(same origin /api/*)"}</div>
            {apiHealthError ? (
              <span className="mt-1 block normal-case text-[#D4A843]" title={apiHealthError}>
                API health failed — check API is up and CORS
              </span>
            ) : !postgresOk ? (
              <span className="mt-1 block normal-case text-[#D4A843]">
                {!postgresConfigured
                  ? "Postgres off — add HSM_COMPANY_OS_DATABASE_URL to repo .env, restart hsm_console"
                  : "Postgres not responding — check DB is running"}
              </span>
            ) : null}
          </div>
        </aside>
      ) : (
        <aside
          className="flex h-full min-h-0 w-12 shrink-0 flex-col items-center overflow-hidden border-r border-[#222222] bg-[#111111] py-2"
          aria-label="Workspace navigation collapsed"
        >
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            className="border border-transparent text-[#888888] hover:bg-white/[0.06] hover:text-[#e8e8e8]"
            title="Expand workspace navigation"
            onClick={() => setSidebarOpen(true)}
          >
            <PanelLeftOpen className="size-4" strokeWidth={1.5} aria-hidden />
            <span className="sr-only">Expand workspace navigation</span>
          </Button>
          <Link
            href="/workspace/dashboard"
            title="Dashboard"
            className="mt-3 flex size-9 items-center justify-center rounded-md text-[#888888] outline-none ring-offset-black hover:bg-white/[0.06] hover:text-[#e8e8e8] focus-visible:ring-2 focus-visible:ring-[#333333]"
          >
            <LayoutDashboard className="size-5 shrink-0" strokeWidth={1.5} aria-hidden />
            <span className="sr-only">Dashboard</span>
          </Link>
          <Link
            href="/workspace/issues"
            title="Issues"
            className="mt-1 flex size-9 items-center justify-center rounded-md text-[#888888] outline-none ring-offset-black hover:bg-white/[0.06] hover:text-[#e8e8e8] focus-visible:ring-2 focus-visible:ring-[#333333]"
          >
            <FolderKanban className="size-5 shrink-0" strokeWidth={1.5} aria-hidden />
            <span className="sr-only">Issues</span>
          </Link>
          <Link
            href="/workspace/agents"
            title="Agents"
            className="mt-1 flex size-9 items-center justify-center rounded-md text-[#888888] outline-none ring-offset-black hover:bg-white/[0.06] hover:text-[#e8e8e8] focus-visible:ring-2 focus-visible:ring-[#333333]"
          >
            <Bot className="size-5 shrink-0" strokeWidth={1.5} aria-hidden />
            <span className="sr-only">Agents</span>
          </Link>
        </aside>
      )}

      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
        <header className="flex h-14 shrink-0 items-center gap-3 border-b border-[#222222] bg-black px-4">
          {!sidebarOpen ? (
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              className="shrink-0 border border-transparent text-[#999999] hover:bg-white/[0.06] hover:text-[#e8e8e8] sm:inline-flex"
              title="Show workspace navigation"
              onClick={() => setSidebarOpen(true)}
            >
              <PanelLeftOpen className="size-4" strokeWidth={1.5} aria-hidden />
              <span className="sr-only">Show workspace navigation</span>
            </Button>
          ) : null}
          {onAgentDetail ? (
            <AgentDetailBreadcrumbs />
          ) : (
            <Breadcrumb className="min-w-0 flex-1">
              <BreadcrumbTrail crumbs={defaultCrumbs} />
            </Breadcrumb>
          )}

          <div className="flex w-[min(100%,220px)] shrink-0 items-center gap-2">
            <span className="sr-only">Company</span>
            <Select
              value={companyId ?? ""}
              onValueChange={(v) => setCompanyId(v || null)}
              disabled={companiesLoading || companies.length === 0}
            >
              <SelectTrigger className="h-9 border-[#333333] bg-[#111111] font-mono text-xs text-[#e8e8e8]">
                <SelectValue placeholder={companiesLoading ? "[LOADING…]" : "Company…"} />
              </SelectTrigger>
              <SelectContent className="border-[#333333] bg-[#111111]">
                {companies.map((c) => (
                  <SelectItem key={c.id} value={c.id} className="font-mono text-xs">
                    {c.display_name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <Button
            type="button"
            variant="outline"
            size="sm"
            className="hidden border-[#333333] bg-[#111111] font-mono text-[11px] uppercase tracking-wide text-[#e8e8e8] hover:bg-white/[0.06] sm:inline-flex"
            onClick={() => setCmdOpen(true)}
          >
            <CommandIcon className="size-3.5" strokeWidth={1.5} />
            <span className="ml-1">⌘K</span>
          </Button>

          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            className="border border-transparent text-[#999999] hover:bg-white/[0.06] hover:text-[#e8e8e8]"
            title={propsOpen ? "Hide agents rail" : "Show agents rail"}
            onClick={() => setPropsOpen((o) => !o)}
          >
            {propsOpen ? <PanelRightClose className="size-4" /> : <PanelRight className="size-4" />}
          </Button>
        </header>

        <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden">
          <main className="pc-workspace-canvas nd-ws-main min-h-0 flex-1 overflow-y-auto overscroll-contain p-4 md:p-6 md:pl-8 md:pr-8">
            <WorkspaceHubLinks />
            {children}
          </main>

          {propsOpen ? (
            <aside className="hidden h-full min-h-0 w-[22rem] shrink-0 flex-col overflow-hidden border-l border-[#222222] bg-[#111111] lg:flex">
              <WorkspaceRightRail />
            </aside>
          ) : null}
        </div>
      </div>

      <CommandDialog open={cmdOpen} onOpenChange={setCmdOpen} title="Command palette" description="Jump to workspace">
        <CommandInput placeholder="Search pages…" className="font-mono text-sm" />
        <CommandList>
          <CommandEmpty>No results.</CommandEmpty>
          <CommandGroup heading="Work">
            <CommandItem onSelect={() => runCommand(() => router.push("/workspace/dashboard"))}>Dashboard</CommandItem>
            <CommandItem onSelect={() => runCommand(() => router.push("/workspace/issues"))}>Issues</CommandItem>
            <CommandItem onSelect={() => runCommand(() => router.push("/workspace/approvals"))}>Approvals</CommandItem>
            <CommandItem onSelect={() => runCommand(() => router.push("/workspace/my-work"))}>My work</CommandItem>
          </CommandGroup>
          <CommandSeparator />
          <CommandGroup heading="Team & procedures">
            <CommandItem onSelect={() => runCommand(() => router.push("/workspace/agents"))}>Agents</CommandItem>
            <CommandItem onSelect={() => runCommand(() => router.push("/workspace/playbooks"))}>Playbooks</CommandItem>
            <CommandItem onSelect={() => runCommand(() => router.push("/workspace/marketplace"))}>Marketplace</CommandItem>
          </CommandGroup>
          <CommandSeparator />
          <CommandGroup heading="Insights">
            <CommandItem onSelect={() => runCommand(() => router.push("/workspace/intelligence"))}>Intelligence</CommandItem>
            <CommandItem onSelect={() => runCommand(() => router.push("/workspace/costs"))}>Costs</CommandItem>
            <CommandItem onSelect={() => runCommand(() => router.push("/workspace/graph"))}>Graph</CommandItem>
            <CommandItem onSelect={() => runCommand(() => router.push("/workspace/architecture"))}>Architecture</CommandItem>
          </CommandGroup>
          <CommandSeparator />
          <CommandGroup heading="Other">
            <CommandItem onSelect={() => runCommand(() => router.push("/"))}>Legacy console</CommandItem>
          </CommandGroup>
          <CommandSeparator />
          <CommandGroup heading="Companies">
            {companies.map((c) => (
              <CommandItem
                key={c.id}
                onSelect={() =>
                  runCommand(() => {
                    setCompanyId(c.id);
                    router.push("/workspace/dashboard");
                  })
                }
              >
                {c.display_name}
              </CommandItem>
            ))}
          </CommandGroup>
        </CommandList>
      </CommandDialog>
    </div>
  );
}
