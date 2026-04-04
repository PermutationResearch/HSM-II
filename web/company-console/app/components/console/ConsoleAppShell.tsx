"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useCallback, useEffect, useState } from "react";
import {
  Bot,
  BrainCircuit,
  CircleDollarSign,
  Command as CommandIcon,
  FolderKanban,
  LayoutDashboard,
  Layers,
  Network,
  PanelRight,
  PanelRightClose,
  ShieldCheck,
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

const nav = [
  { href: "/workspace/dashboard", label: "Dashboard", icon: LayoutDashboard },
  { href: "/workspace/agents", label: "Agents", icon: Bot },
  { href: "/workspace/issues", label: "Issues", icon: FolderKanban },
  { href: "/workspace/approvals", label: "Approvals", icon: ShieldCheck },
  { href: "/workspace/costs", label: "Costs", icon: CircleDollarSign },
  { href: "/workspace/graph", label: "Graph", icon: Network },
  { href: "/workspace/architecture", label: "Architecture", icon: Layers },
  { href: "/workspace/intelligence", label: "Intelligence", icon: BrainCircuit },
] as const;

type Crumb = { label: string; href: string | null };

function breadcrumbsForPath(pathname: string): Crumb[] {
  const root: Crumb = { label: "Workspace", href: "/workspace/dashboard" };
  if (pathname.startsWith("/workspace/dashboard")) return [root, { label: "Dashboard", href: null }];
  if (pathname.startsWith("/workspace/agents")) return [root, { label: "Agents", href: null }];
  if (pathname.startsWith("/workspace/issues")) return [root, { label: "Issues", href: null }];
  if (pathname.startsWith("/workspace/approvals")) return [root, { label: "Approvals", href: null }];
  if (pathname.startsWith("/workspace/costs")) return [root, { label: "Costs", href: null }];
  if (pathname.startsWith("/workspace/graph")) return [root, { label: "Graph", href: null }];
  if (pathname.startsWith("/workspace/architecture")) return [root, { label: "Architecture", href: null }];
  if (pathname.startsWith("/workspace/intelligence")) return [root, { label: "Intelligence", href: null }];
  return [root];
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
    propertiesSelection,
    setPropertiesSelection,
  } = useWorkspace();

  const [cmdOpen, setCmdOpen] = useState(false);
  const [propsOpen, setPropsOpen] = useState(true);

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

  const crumbs = breadcrumbsForPath(pathname ?? "");

  const runCommand = useCallback(
    (fn: () => void) => {
      setCmdOpen(false);
      fn();
    },
    [],
  );

  return (
    <div className="flex min-h-screen bg-admin-bg text-foreground">
      <aside className="flex w-56 shrink-0 flex-col border-r border-admin-border bg-admin-panel">
        <div className="border-b border-admin-border px-3 py-3">
          <Link href="/workspace/dashboard" className="pc-sidebar-brand">
            Company console
          </Link>
          <p className="mt-1 text-[10px] leading-snug text-admin-muted">Paperclip-class workspace</p>
        </div>
        <nav className="flex flex-1 flex-col gap-0.5 p-2">
          {nav.map(({ href, label, icon: Icon }) => {
            const active = pathname === href || (href !== "/workspace/dashboard" && pathname?.startsWith(href));
            return (
              <Link
                key={href}
                href={href}
                className={cn(
                  "flex items-center gap-2 rounded-md px-2 py-2 text-sm transition-colors",
                  active ? "bg-primary/15 text-primary" : "text-admin-muted hover:bg-white/5 hover:text-foreground",
                )}
              >
                <Icon className="size-4 shrink-0 opacity-80" strokeWidth={1.5} />
                {label}
              </Link>
            );
          })}
          <Separator className="my-2 bg-admin-border" />
          <Link
            href="/"
            className="rounded-md px-2 py-2 text-sm text-admin-muted hover:bg-white/5 hover:text-foreground"
          >
            Legacy full console →
          </Link>
        </nav>
        <div className="border-t border-admin-border p-2 text-[10px] text-admin-muted">
          API: <span className="font-mono text-[9px] text-muted-foreground">{apiBase}</span>
          {!postgresOk ? <span className="mt-1 block text-warn">Postgres not ready</span> : null}
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center gap-3 border-b border-admin-border bg-admin-bg px-4">
          <Breadcrumb className="min-w-0 flex-1">
            <BreadcrumbList className="flex-nowrap">
              {crumbs.map((c, i) => (
                <span key={`${c.label}-${i}`} className="contents">
                  {i > 0 ? <BreadcrumbSeparator /> : null}
                  <BreadcrumbItem>
                    {c.href ? (
                      <BreadcrumbLink asChild>
                        <Link href={c.href}>{c.label}</Link>
                      </BreadcrumbLink>
                    ) : (
                      <BreadcrumbPage>{c.label}</BreadcrumbPage>
                    )}
                  </BreadcrumbItem>
                </span>
              ))}
            </BreadcrumbList>
          </Breadcrumb>

          <div className="flex w-[min(100%,220px)] shrink-0 items-center gap-2">
            <span className="sr-only">Company</span>
            <Select
              value={companyId ?? ""}
              onValueChange={(v) => setCompanyId(v || null)}
              disabled={companiesLoading || companies.length === 0}
            >
              <SelectTrigger className="h-9 border-admin-border bg-admin-panel font-mono text-xs">
                <SelectValue placeholder="Company…" />
              </SelectTrigger>
              <SelectContent className="border-admin-border bg-admin-panel">
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
            className="hidden border-admin-border bg-admin-panel font-mono text-xs sm:inline-flex"
            onClick={() => setCmdOpen(true)}
          >
            <CommandIcon className="size-3.5" />
            <span className="ml-1">⌘K</span>
          </Button>

          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            className="border border-transparent text-admin-muted hover:bg-white/5"
            title={propsOpen ? "Hide properties" : "Show properties"}
            onClick={() => setPropsOpen((o) => !o)}
          >
            {propsOpen ? <PanelRightClose className="size-4" /> : <PanelRight className="size-4" />}
          </Button>
        </header>

        <div className="flex min-h-0 flex-1">
          <main className="pc-workspace-canvas nd-dashboard-shell p-4 md:p-6">{children}</main>

          {propsOpen ? (
            <aside className="hidden w-72 shrink-0 border-l border-admin-border bg-admin-panel/80 lg:block">
              <div className="border-b border-admin-border px-3 py-2 font-mono text-[11px] font-semibold uppercase tracking-wide text-admin-muted">
                Properties
              </div>
              <div className="p-3 text-sm text-muted-foreground">
                {!propertiesSelection ? (
                  <p className="text-xs leading-relaxed">
                    Select a task or agent on Issues/Agents to pin details here. Command palette:{" "}
                    <kbd className="rounded border border-admin-border px-1 font-mono text-[10px]">⌘K</kbd>
                  </p>
                ) : propertiesSelection.kind === "task" ? (
                  <div className="space-y-2">
                    <p className="font-mono text-[10px] uppercase text-admin-muted">Task</p>
                    <p className="font-medium text-foreground">{propertiesSelection.title ?? propertiesSelection.id}</p>
                    <p className="break-all font-mono text-xs text-muted-foreground">{propertiesSelection.id}</p>
                    <Button variant="outline" size="xs" className="mt-2" onClick={() => setPropertiesSelection(null)}>
                      Clear
                    </Button>
                  </div>
                ) : (
                  <div className="space-y-2">
                    <p className="font-mono text-[10px] uppercase text-admin-muted">Agent</p>
                    <p className="font-medium text-foreground">{propertiesSelection.name ?? propertiesSelection.id}</p>
                    <p className="break-all font-mono text-xs text-muted-foreground">{propertiesSelection.id}</p>
                    <Button variant="outline" size="xs" className="mt-2" onClick={() => setPropertiesSelection(null)}>
                      Clear
                    </Button>
                  </div>
                )}
              </div>
            </aside>
          ) : null}
        </div>
      </div>

      <CommandDialog open={cmdOpen} onOpenChange={setCmdOpen} title="Command palette" description="Navigate workspace">
        <CommandInput placeholder="Search pages and actions…" />
        <CommandList>
          <CommandEmpty>No results.</CommandEmpty>
          <CommandGroup heading="Navigate">
            <CommandItem onSelect={() => runCommand(() => router.push("/workspace/dashboard"))}>Dashboard</CommandItem>
            <CommandItem onSelect={() => runCommand(() => router.push("/workspace/agents"))}>Agents</CommandItem>
            <CommandItem onSelect={() => runCommand(() => router.push("/workspace/issues"))}>Issues</CommandItem>
            <CommandItem onSelect={() => runCommand(() => router.push("/workspace/approvals"))}>Approvals</CommandItem>
            <CommandItem onSelect={() => runCommand(() => router.push("/workspace/costs"))}>Costs</CommandItem>
            <CommandItem onSelect={() => runCommand(() => router.push("/workspace/graph"))}>Graph</CommandItem>
            <CommandItem onSelect={() => runCommand(() => router.push("/workspace/architecture"))}>Architecture</CommandItem>
            <CommandItem onSelect={() => runCommand(() => router.push("/workspace/intelligence"))}>Intelligence</CommandItem>
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
