"use client";

/**
 * Layout and IA ported from Paperclip's open-source dashboard:
 * https://github.com/paperclipai/paperclip/blob/master/ui/src/pages/Dashboard.tsx
 *
 * Data comes from HSM `hsm_console` (Company OS). We do not ship Paperclip's proprietary backend;
 * the UI is adapted so metrics, agents, issues, and activity map to our APIs.
 *
 * Widget grid powered by react-grid-layout:
 * - CSS transform: translate() during drag (GPU composited, no reflow)
 * - will-change: transform for compositor layer promotion
 * - requestAnimationFrame-driven resize via RGL internals
 * - Lightweight resize handles (corner only)
 * - Modern pointer events (mouse, touch, pen)
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Bot,
  CircleDot,
  DollarSign,
  LayoutDashboard,
  Lock,
  PauseCircle,
  ShieldCheck,
  Unlock,
} from "lucide-react";
import { ResponsiveGridLayout, useContainerWidth, verticalCompactor, type Layout, type ResponsiveLayouts } from "react-grid-layout";
import { transformStrategy } from "react-grid-layout/core";

type Layouts = ResponsiveLayouts;
import { ActiveAgentsPanel } from "../components/ActiveAgentsPanel";
import {
  ChartCard,
  IssueStatusChart,
  PriorityChart,
  RunActivityChart,
  SuccessRateChart,
  tasksInRunActivityDayBucket,
} from "../components/ActivityCharts";
import { ActivityRow, type ActivityEvent, type AgentLite } from "../components/ActivityRow";
import { EmptyState } from "../components/EmptyState";
import { Identity } from "../components/Identity";
import { MetricCard } from "../components/MetricCard";
import { PageSkeleton } from "../components/PageSkeleton";
import { StatusIcon } from "../components/StatusIcon";
import { useHsmCompanyDashboard, type HsmGovEvent, type HsmTask } from "../hooks/useHsmCompanyDashboard";
import { timeAgo } from "../lib/timeAgo";
import { formatCents } from "../lib/utils";


export type HsmCompanyOption = { id: string; display_name: string; issue_key_prefix?: string };

/** Queue tabs on Inbox & tasks — must match `QueueView` in PolicyQueuePanel / API. */
export type DashboardDrillQueueView =
  | "all"
  | "overdue"
  | "atrisk"
  | "waiting_admin"
  | "pending_approvals"
  | "blocked";

export type DashboardDrillDown =
  | { type: "inbox" }
  | { type: "queue"; view: DashboardDrillQueueView }
  | { type: "task"; taskId: string }
  | { type: "persona"; persona: string }
  | { type: "filter_priority"; level: 0 | 1 | 2 | 3 }
  | { type: "filter_state"; state: string }
  | { type: "filter_task_ids"; ids: string[] }
  | { type: "filter_in_progress" }
  | { type: "filter_open" }
  | { type: "filter_blocked" }
  | { type: "filter_completed" }
  | { type: "spend" };

export type DashboardProps = {
  apiBase: string;
  companyId: string | null;
  companies: HsmCompanyOption[];
  hrefAgents?: string;
  hrefTasks?: string;
  hrefCosts?: string;
  hrefApprovals?: string;
  onOpenOnboarding?: () => void;
  layout?: "nothing" | "admin";
  onDrillDown?: (action: DashboardDrillDown) => void;
};

type IssueLike = {
  id: string;
  title: string;
  status: string;
  updatedAt: string;
  identifier?: string;
  assigneeAgentId?: string | null;
};

function getRecentIssues(issues: IssueLike[]): IssueLike[] {
  return [...issues].sort((a, b) => new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime());
}

function taskIdentifier(t: HsmTask, issueKeyPrefix: string | null | undefined): string {
  if (typeof t.display_number === "number") {
    const p = (issueKeyPrefix ?? "HSM").toUpperCase();
    return `${p}-${t.display_number}`;
  }
  return `HSM-${t.id.slice(0, 8)}`;
}

function taskToIssue(t: HsmTask, issueKeyPrefix: string | null | undefined): IssueLike {
  return {
    id: t.id,
    title: t.title,
    status: t.state,
    updatedAt: t.due_at ?? new Date().toISOString(),
    identifier: taskIdentifier(t, issueKeyPrefix),
    assigneeAgentId: t.owner_persona ?? t.checked_out_by ?? null,
  };
}

function govToActivity(e: HsmGovEvent): ActivityEvent {
  let details: Record<string, unknown> | null = null;
  if (e.payload && typeof e.payload === "object" && !Array.isArray(e.payload)) {
    details = e.payload as Record<string, unknown>;
  }
  return {
    id: e.id,
    action: `governance.${e.action}`,
    createdAt: e.created_at,
    actorId: e.actor,
    actorType: "user",
    entityType: e.subject_type,
    entityId: e.subject_id,
    details,
  };
}

/* ── Grid layout definitions ── */

const LAYOUT_STORAGE_KEY = "hsm-dashboard-grid-layout-v4";

const DEFAULT_LAYOUTS: Layouts = {
  lg: [
    { i: "metric-agents",    x: 0, y: 0, w: 3, h: 3, minW: 2, minH: 3 },
    { i: "metric-tasks",     x: 3, y: 0, w: 3, h: 3, minW: 2, minH: 3 },
    { i: "metric-spend",     x: 6, y: 0, w: 3, h: 3, minW: 2, minH: 3 },
    { i: "metric-approvals", x: 9, y: 0, w: 3, h: 3, minW: 2, minH: 3 },
    { i: "chart-activity",   x: 0, y: 3, w: 3, h: 7, minW: 2, minH: 6 },
    { i: "chart-priority",   x: 3, y: 3, w: 3, h: 7, minW: 2, minH: 6 },
    { i: "chart-status",     x: 6, y: 3, w: 3, h: 7, minW: 2, minH: 6 },
    { i: "chart-success",    x: 9, y: 3, w: 3, h: 7, minW: 2, minH: 6 },
    { i: "recent-activity",  x: 0, y: 10, w: 6, h: 11, minW: 3, minH: 8 },
    { i: "recent-tasks",     x: 6, y: 10, w: 6, h: 11, minW: 3, minH: 8 },
  ],
  md: [
    { i: "metric-agents",    x: 0, y: 0, w: 5, h: 3, minW: 2, minH: 3 },
    { i: "metric-tasks",     x: 5, y: 0, w: 5, h: 3, minW: 2, minH: 3 },
    { i: "metric-spend",     x: 0, y: 3, w: 5, h: 3, minW: 2, minH: 3 },
    { i: "metric-approvals", x: 5, y: 3, w: 5, h: 3, minW: 2, minH: 3 },
    { i: "chart-activity",   x: 0, y: 6, w: 5, h: 7, minW: 2, minH: 6 },
    { i: "chart-priority",   x: 5, y: 6, w: 5, h: 7, minW: 2, minH: 6 },
    { i: "chart-status",     x: 0, y: 13, w: 5, h: 7, minW: 2, minH: 6 },
    { i: "chart-success",    x: 5, y: 13, w: 5, h: 7, minW: 2, minH: 6 },
    { i: "recent-activity",  x: 0, y: 20, w: 5, h: 11, minW: 3, minH: 8 },
    { i: "recent-tasks",     x: 5, y: 20, w: 5, h: 11, minW: 3, minH: 8 },
  ],
  sm: [
    { i: "metric-agents",    x: 0, y: 0, w: 6, h: 3, minW: 2, minH: 3 },
    { i: "metric-tasks",     x: 0, y: 3, w: 6, h: 3, minW: 2, minH: 3 },
    { i: "metric-spend",     x: 0, y: 6, w: 6, h: 3, minW: 2, minH: 3 },
    { i: "metric-approvals", x: 0, y: 9, w: 6, h: 3, minW: 2, minH: 3 },
    { i: "chart-activity",   x: 0, y: 12, w: 6, h: 7, minW: 2, minH: 6 },
    { i: "chart-priority",   x: 0, y: 19, w: 6, h: 7, minW: 2, minH: 6 },
    { i: "chart-status",     x: 0, y: 26, w: 6, h: 7, minW: 2, minH: 6 },
    { i: "chart-success",    x: 0, y: 33, w: 6, h: 7, minW: 2, minH: 6 },
    { i: "recent-activity",  x: 0, y: 40, w: 6, h: 11, minW: 3, minH: 8 },
    { i: "recent-tasks",     x: 0, y: 51, w: 6, h: 11, minW: 3, minH: 8 },
  ],
};

function loadSavedLayouts(): Layouts | null {
  if (typeof window === "undefined") return null;
  try {
    const raw = localStorage.getItem(LAYOUT_STORAGE_KEY);
    if (raw) return JSON.parse(raw) as Layouts;
    const legacy = localStorage.getItem("hsm-dashboard-grid-layout");
    if (legacy) return JSON.parse(legacy) as Layouts;
  } catch { /* ignore */ }
  return null;
}

function saveLayouts(layouts: Layouts) {
  if (typeof window === "undefined") return;
  try {
    localStorage.setItem(LAYOUT_STORAGE_KEY, JSON.stringify(layouts));
  } catch { /* ignore */ }
}

function serializeLayouts(layouts: Layouts): string {
  return JSON.stringify(layouts);
}

/* ── Widget wrapper ── */

function WidgetShell({
  children,
  className,
  dragHandleClass,
  editing,
}: {
  children: React.ReactNode;
  className?: string;
  dragHandleClass: string;
  /** When false, handle is non-interactive so links/buttons under the corner keep working. */
  editing: boolean;
}) {
  return (
    <div
      className={`relative h-full min-h-0 overflow-hidden rounded-2xl border border-[#2a2a2a] bg-[#0a0a0a] ${className ?? ""}`}
    >
      <div
        role="button"
        tabIndex={editing ? 0 : -1}
        aria-label={editing ? "Drag to move widget" : undefined}
        title={editing ? "Drag to move widget" : "Unlock grid to drag widgets"}
        className={`${dragHandleClass} absolute inset-x-3 top-3 z-[40] h-7 select-none rounded-xl bg-transparent transition-opacity duration-150 touch-none ${
          editing
            ? "pointer-events-auto cursor-grab opacity-100 active:cursor-grabbing"
            : "pointer-events-none opacity-0"
        }`}
      />
      <div className="relative z-0 h-full min-h-0 touch-pan-y touch-pinch-zoom overflow-auto p-4">{children}</div>
    </div>
  );
}

/* ── Main component ── */

export function Dashboard({
  apiBase,
  companyId,
  companies,
  hrefAgents,
  hrefTasks,
  hrefCosts,
  hrefApprovals,
  onOpenOnboarding,
  layout = "nothing",
  onDrillDown,
}: DashboardProps) {
  const isAdmin = layout === "admin";
  const { data, loading, error } = useHsmCompanyDashboard(apiBase, companyId);

  const [gridLocked, setGridLocked] = useState(false);
  const [layouts, setLayouts] = useState<Layouts>(() => loadSavedLayouts() ?? DEFAULT_LAYOUTS);
  const layoutsRef = useRef<Layouts>(layouts);
  const layoutInteractionRef = useRef(false);
  const persistedLayoutsRef = useRef(serializeLayouts(layouts));

  const [animatedActivityIds, setAnimatedActivityIds] = useState<Set<string>>(new Set());
  const seenActivityIdsRef = useRef<Set<string>>(new Set());
  const hydratedActivityRef = useRef(false);
  const activityAnimationTimersRef = useRef<number[]>([]);

  const DRAG_HANDLE_CLASS = "dashboard-drag-handle";
  const { containerRef: gridContainerRef, width: gridWidth } = useContainerWidth();

  useEffect(() => {
    layoutsRef.current = layouts;
    persistedLayoutsRef.current = serializeLayouts(layouts);
  }, [layouts]);

  const persistLayouts = useCallback((nextLayouts: Layouts) => {
    const serialized = serializeLayouts(nextLayouts);
    layoutsRef.current = nextLayouts;
    if (serialized === persistedLayoutsRef.current) return;
    persistedLayoutsRef.current = serialized;
    setLayouts(nextLayouts);
    saveLayouts(nextLayouts);
  }, []);

  const handleLayoutChange = useCallback((_current: Layout, allLayouts: Layouts) => {
    layoutsRef.current = allLayouts;
    if (!layoutInteractionRef.current) {
      persistLayouts(allLayouts);
    }
  }, [persistLayouts]);

  const handleLayoutInteractStart = useCallback(() => {
    layoutInteractionRef.current = true;
  }, []);

  const handleLayoutInteractStop = useCallback(() => {
    layoutInteractionRef.current = false;
    persistLayouts(layoutsRef.current);
  }, [persistLayouts]);

  const handleResetLayout = useCallback(() => {
    layoutInteractionRef.current = false;
    persistLayouts(DEFAULT_LAYOUTS);
  }, [persistLayouts]);

  const issueKeyPrefix = useMemo(
    () => companies.find((c) => c.id === companyId)?.issue_key_prefix,
    [companies, companyId]
  );

  const agents = useMemo((): AgentLite[] => {
    const m = new Map<string, AgentLite>();
    for (const a of data?.companyAgents ?? []) {
      if (a.status === "terminated") continue;
      const label = a.title?.trim() ? `${a.name} · ${a.title.trim()}` : a.name;
      m.set(a.name, { id: a.name, name: label, title: a.title ?? undefined, role: a.role });
    }
    for (const t of data?.tasks ?? []) {
      const id = t.owner_persona ?? t.checked_out_by;
      if (!id) continue;
      if (!m.has(id)) m.set(id, { id, name: id });
    }
    return Array.from(m.values());
  }, [data?.tasks, data?.companyAgents]);

  const agentRunRegistry = useMemo(() => {
    const r: Record<string, { title?: string; role?: string }> = {};
    for (const a of data?.companyAgents ?? []) {
      if (a.status === "terminated") continue;
      r[a.name] = { title: a.title ?? undefined, role: a.role };
    }
    return r;
  }, [data?.companyAgents]);

  const issues = useMemo(
    () => (data?.tasks ?? []).map((t) => taskToIssue(t, issueKeyPrefix)),
    [data?.tasks, issueKeyPrefix]
  );

  const activity = useMemo(() => (data?.governance ?? []).map(govToActivity), [data?.governance]);

  const recentIssues = issues ? getRecentIssues(issues) : [];
  const recentActivity = useMemo(() => [...activity].slice(0, 10), [activity]);

  useEffect(() => {
    for (const timer of activityAnimationTimersRef.current) window.clearTimeout(timer);
    activityAnimationTimersRef.current = [];
    seenActivityIdsRef.current = new Set();
    hydratedActivityRef.current = false;
    setAnimatedActivityIds(new Set());
  }, [companyId]);

  useEffect(() => {
    if (recentActivity.length === 0) return;
    const seen = seenActivityIdsRef.current;
    const currentIds = recentActivity.map((event) => event.id);
    if (!hydratedActivityRef.current) {
      for (const id of currentIds) seen.add(id);
      hydratedActivityRef.current = true;
      return;
    }
    const newIds = currentIds.filter((id) => !seen.has(id));
    if (newIds.length === 0) {
      for (const id of currentIds) seen.add(id);
      return;
    }
    setAnimatedActivityIds((prev) => {
      const next = new Set(prev);
      for (const id of newIds) next.add(id);
      return next;
    });
    for (const id of newIds) seen.add(id);
    const timer = window.setTimeout(() => {
      setAnimatedActivityIds((prev) => {
        const next = new Set(prev);
        for (const id of newIds) next.delete(id);
        return next;
      });
      activityAnimationTimersRef.current = activityAnimationTimersRef.current.filter((t) => t !== timer);
    }, 980);
    activityAnimationTimersRef.current.push(timer);
  }, [recentActivity]);

  useEffect(() => {
    return () => {
      for (const timer of activityAnimationTimersRef.current) window.clearTimeout(timer);
    };
  }, []);

  const agentMap = useMemo(() => {
    const map = new Map<string, AgentLite>();
    for (const a of agents) map.set(a.id, a);
    return map;
  }, [agents]);

  const entityNameMap = useMemo(() => {
    const map = new Map<string, string>();
    for (const i of issues ?? []) map.set(`issue:${i.id}`, i.identifier ?? i.id.slice(0, 8));
    for (const a of agents ?? []) map.set(`agent:${a.id}`, a.name);
    for (const g of data?.governance ?? []) {
      map.set(`${g.subject_type}:${g.subject_id}`, g.subject_id.slice(0, 8));
    }
    return map;
  }, [issues, agents, data?.governance]);

  const entityTitleMap = useMemo(() => {
    const map = new Map<string, string>();
    for (const i of issues ?? []) map.set(`issue:${i.id}`, i.title);
    return map;
  }, [issues]);

  const agentName = (id: string | null) => {
    if (!id) return null;
    return agents.find((a) => a.id === id)?.name ?? null;
  };

  const dashboardMetrics = useMemo(() => {
    const tasks = data?.tasks ?? [];
    const open = tasks.filter((t) => /open|todo|pending/i.test(t.state)).length;
    const blocked = tasks.filter((t) => /block/i.test(t.state)).length;
    const inProgress = tasks.filter((t) => /progress|doing|active/i.test(t.state) || !!t.checked_out_by).length;
    const pendingApprovals = tasks.filter((t) => t.decision_mode === "admin_required").length;
    const monthSpendCents = Math.round((data?.spend?.total_usd ?? 0) * 100);
    const monthBudgetCents = (data?.companyAgents ?? [])
      .filter((a) => a.status !== "terminated" && typeof a.budget_monthly_cents === "number")
      .reduce((s, a) => s + Math.max(0, a.budget_monthly_cents ?? 0), 0);
    const monthUtilizationPercent =
      monthBudgetCents > 0 ? Math.min(100, Math.round((monthSpendCents / monthBudgetCents) * 100)) : 0;
    const running = tasks.filter((t) => !!t.checked_out_by).length;
    const err = tasks.filter((t) => /fail|error/i.test(t.state)).length;
    const pausedAgents = (data?.companyAgents ?? []).filter((a) => a.status === "paused").length;
    const activeAgents = agents.length;
    const totalEnabled = activeAgents + running > 0 ? activeAgents + running : tasks.length > 0 ? 1 : 0;
    return {
      agents: { active: activeAgents, running, paused: pausedAgents, error: err },
      totalEnabled,
      tasks: { inProgress, open, blocked },
      costs: { monthSpendCents, monthBudgetCents, monthUtilizationPercent },
      budgets: {
        activeIncidents: blocked > 0 ? 1 : 0,
        pausedAgents,
        pausedProjects: 0,
        pendingApprovals,
      },
      pendingApprovals,
    };
  }, [data?.tasks, data?.spend, data?.companyAgents, agents]);

  if (!companyId) {
    if (companies.length === 0) {
      return (
        <EmptyState
          icon={LayoutDashboard}
          message="Welcome to HSM Console. Set up PostgreSQL Company OS and create a company to get started."
          action={onOpenOnboarding ? "Get started" : undefined}
          onAction={onOpenOnboarding}
        />
      );
    }
    return <EmptyState icon={LayoutDashboard} message="Create or select a company to view the dashboard." />;
  }

  if (loading && !data) {
    return <PageSkeleton variant="dashboard" />;
  }

  const dm = dashboardMetrics;
  const runs = data?.tasks ?? [];
  const companyLabel = companies.find((c) => c.id === companyId)?.display_name ?? null;

  const drill = onDrillDown;
  const metricTo = (fallback?: string) => (drill ? undefined : fallback);
  const metricClick = (fn: () => void) => (drill ? fn : undefined);

  return (
    <div className="-mx-6 space-y-6 bg-[#010409] px-6 pb-10">
      <header className="flex flex-col gap-3 border-b border-[#30363D] pb-6 sm:flex-row sm:items-start sm:justify-between sm:gap-4">
        <div className="min-w-0">
          <h1 className="text-lg font-medium tracking-tight text-white">Dashboard</h1>
          <p className="mt-1 text-sm text-[#8B949E]">
            {companyLabel ?? "Company workspace"} · multi-agent overview
          </p>
        </div>
        <div className="flex items-center gap-3">
          {dm.agents.running > 0 ? (
            <span className="inline-flex w-fit shrink-0 items-center rounded-md bg-[#388bfd]/12 px-2.5 py-1 font-mono text-[11px] font-semibold uppercase tracking-wide text-[#79c0ff]">
              {dm.agents.running} live
            </span>
          ) : (
            <span className="font-mono text-[11px] uppercase tracking-wide text-[#484f58]">No live runs</span>
          )}
          <button
            type="button"
            onClick={() => setGridLocked((v) => !v)}
            className="inline-flex items-center gap-1.5 rounded-md border border-[#30363D] px-2 py-1 font-mono text-[10px] uppercase tracking-wide text-[#8B949E] transition-colors hover:border-[#58a6ff]/40 hover:text-[#58a6ff]"
            title={gridLocked ? "Unlock grid to drag & resize widgets" : "Lock grid layout"}
          >
            {gridLocked ? <Lock className="h-3 w-3" /> : <Unlock className="h-3 w-3" />}
            {gridLocked ? "Locked" : "Editing"}
          </button>
          {!gridLocked && (
            <button
              type="button"
              onClick={handleResetLayout}
              className="rounded-md border border-[#30363D] px-2 py-1 font-mono text-[10px] uppercase tracking-wide text-[#8B949E] transition-colors hover:border-[#d71921]/40 hover:text-[#d71921]"
            >
              Reset
            </button>
          )}
        </div>
      </header>

      {error && <p className="text-sm text-destructive">{error}</p>}

      {(data?.companyAgents?.length === 0 && (data?.tasks.length ?? 0) === 0) && (
        <div className="flex items-center justify-between gap-3 rounded-2xl border border-[#D4A843] bg-card px-4 py-3">
          <div className="flex items-center gap-2.5">
            <Bot className="h-4 w-4 shrink-0 text-[#D4A843]" strokeWidth={1.5} />
            <p className="text-sm text-[#E8E8E8]">You have no agents.</p>
          </div>
          {hrefAgents ? (
            <a
              href={hrefAgents}
              className="shrink-0 font-mono text-xs uppercase tracking-wide text-[#5B9BF6] underline underline-offset-4 hover:text-white"
            >
              Create one here
            </a>
          ) : null}
        </div>
      )}

      <ActiveAgentsPanel
        tasks={data?.tasks ?? []}
        layout={layout}
        agentRegistry={agentRunRegistry}
        issueKeyPrefix={issueKeyPrefix}
        onTaskClick={drill ? (taskId) => drill({ type: "task", taskId }) : undefined}
      />

      {dm.budgets.activeIncidents > 0 ? (
        <div className="flex items-start justify-between gap-3 rounded-2xl border border-[#D71921] bg-card px-4 py-3">
          <div className="flex items-start gap-2.5">
            <PauseCircle className="mt-0.5 h-4 w-4 shrink-0 text-[#D71921]" strokeWidth={1.5} />
            <div>
              <p className="text-sm font-medium text-white">
                {dm.budgets.activeIncidents} active budget incident{dm.budgets.activeIncidents === 1 ? "" : "s"}
              </p>
              <p className="mt-1 font-mono text-[11px] uppercase tracking-wide text-[#999999]">
                {dm.budgets.pausedAgents} agents paused · {dm.budgets.pausedProjects} projects paused ·{" "}
                {dm.budgets.pendingApprovals} pending budget approvals
              </p>
            </div>
          </div>
          {hrefCosts ? (
            <a
              href={hrefCosts}
              className="font-mono text-xs uppercase tracking-wide text-[#5B9BF6] underline underline-offset-4"
            >
              Open budgets
            </a>
          ) : null}
        </div>
      ) : null}

      {data && (
        <div ref={gridContainerRef as React.Ref<HTMLDivElement>}>
        <ResponsiveGridLayout
          className="layout"
          layouts={layouts}
          breakpoints={{ lg: 1200, md: 768, sm: 0 }}
          cols={{ lg: 12, md: 10, sm: 6 }}
          autoSize
          rowHeight={44}
          width={gridWidth}
          positionStrategy={transformStrategy}
          dragConfig={{
            enabled: !gridLocked,
            handle: `.${DRAG_HANDLE_CLASS}`,
            cancel:
              "input,textarea,button,select,option,.react-resizable-handle,[data-dashboard-no-drag]",
          }}
          resizeConfig={{ enabled: !gridLocked, handles: ["n", "e", "s", "w", "ne", "nw", "se", "sw"] }}
          compactor={verticalCompactor}
          onDragStart={handleLayoutInteractStart}
          onResizeStart={handleLayoutInteractStart}
          onDragStop={handleLayoutInteractStop}
          onResizeStop={handleLayoutInteractStop}
          onLayoutChange={handleLayoutChange}
          margin={[12, 12]}
        >
          {/* ── Metric cards ── */}
          <div key="metric-agents">
            <WidgetShell dragHandleClass={DRAG_HANDLE_CLASS} editing={!gridLocked}>
              <MetricCard
                icon={Bot}
                value={dm.totalEnabled}
                label="Agents Enabled"
                variant="admin"
                to={metricTo(hrefAgents)}
                onClick={metricClick(() => drill?.({ type: "inbox" }))}
                description={
                  <span>
                    {dm.agents.running} running{", "}
                    {dm.agents.paused} paused{", "}
                    {dm.agents.error} errors
                  </span>
                }
              />
            </WidgetShell>
          </div>
          <div key="metric-tasks">
            <WidgetShell dragHandleClass={DRAG_HANDLE_CLASS} editing={!gridLocked}>
              <MetricCard
                icon={CircleDot}
                value={dm.tasks.inProgress}
                label="Tasks In Progress"
                variant="admin"
                to={metricTo(hrefTasks)}
                onClick={metricClick(() => drill?.({ type: "filter_in_progress" }))}
                description={
                  <span>
                    {dm.tasks.open} open{", "}
                    {dm.tasks.blocked} blocked
                  </span>
                }
              />
            </WidgetShell>
          </div>
          <div key="metric-spend">
            <WidgetShell dragHandleClass={DRAG_HANDLE_CLASS} editing={!gridLocked}>
              <MetricCard
                icon={DollarSign}
                value={formatCents(dm.costs.monthSpendCents)}
                label="Month Spend"
                variant="admin"
                to={metricTo(hrefCosts)}
                onClick={metricClick(() => drill?.({ type: "spend" }))}
                description={
                  <span>
                    {dm.costs.monthBudgetCents > 0
                      ? `${dm.costs.monthUtilizationPercent}% of ${formatCents(dm.costs.monthBudgetCents)} budget`
                      : "Unlimited budget"}
                  </span>
                }
              />
            </WidgetShell>
          </div>
          <div key="metric-approvals">
            <WidgetShell dragHandleClass={DRAG_HANDLE_CLASS} editing={!gridLocked}>
              <MetricCard
                icon={ShieldCheck}
                value={dm.pendingApprovals + dm.budgets.pendingApprovals}
                label="Pending Approvals"
                variant="admin"
                to={metricTo(hrefApprovals)}
                onClick={metricClick(() => drill?.({ type: "queue", view: "pending_approvals" }))}
                description={
                  <span>
                    {dm.budgets.pendingApprovals > 0
                      ? `${dm.budgets.pendingApprovals} budget overrides awaiting board review`
                      : "Awaiting board review"}
                  </span>
                }
              />
            </WidgetShell>
          </div>

          {/* ── Chart widgets ── */}
          <div key="chart-activity">
            <WidgetShell dragHandleClass={DRAG_HANDLE_CLASS} editing={!gridLocked}>
              <ChartCard title="Run Activity" subtitle="Last 14 days" layout="admin">
                {drill ? (
                  <p className="mb-2 font-mono text-[10px] uppercase tracking-wide text-[#484f58]">
                    Bar → tasks updated on that day
                  </p>
                ) : null}
                <RunActivityChart
                  tasks={runs}
                  variant="admin"
                  onDayClick={
                    drill
                      ? (dayIndex) =>
                          drill({
                            type: "filter_task_ids",
                            ids: tasksInRunActivityDayBucket(runs, dayIndex).map((t) => t.id),
                          })
                      : undefined
                  }
                />
              </ChartCard>
            </WidgetShell>
          </div>
          <div key="chart-priority">
            <WidgetShell dragHandleClass={DRAG_HANDLE_CLASS} editing={!gridLocked}>
              <ChartCard title="Issues by Priority" subtitle="Last 14 days" layout="admin">
                {drill ? (
                  <p className="mb-2 font-mono text-[10px] uppercase tracking-wide text-[#484f58]">
                    Bar → tasks at that priority
                  </p>
                ) : null}
                <PriorityChart
                  tasks={runs}
                  onPriorityClick={drill ? (level) => drill({ type: "filter_priority", level }) : undefined}
                />
              </ChartCard>
            </WidgetShell>
          </div>
          <div key="chart-status">
            <WidgetShell dragHandleClass={DRAG_HANDLE_CLASS} editing={!gridLocked}>
              <ChartCard title="Issues by Status" subtitle="Last 14 days" layout="admin">
                {drill ? (
                  <p className="mb-2 font-mono text-[10px] uppercase tracking-wide text-[#484f58]">
                    Bar → tasks in that status
                  </p>
                ) : null}
                <IssueStatusChart tasks={runs} onStatusClick={drill ? (state) => drill({ type: "filter_state", state }) : undefined} />
              </ChartCard>
            </WidgetShell>
          </div>
          <div key="chart-success">
            <WidgetShell dragHandleClass={DRAG_HANDLE_CLASS} editing={!gridLocked}>
              <ChartCard title="Success Rate" subtitle="Last 14 days" layout="admin">
                {drill ? (
                  <p className="mb-2 font-mono text-[10px] uppercase tracking-wide text-[#484f58]">
                    Bar → daily run success rate
                  </p>
                ) : null}
                <SuccessRateChart tasks={runs} onCompletedClick={drill ? () => drill({ type: "filter_completed" }) : undefined} />
              </ChartCard>
            </WidgetShell>
          </div>

          {/* ── Feed widgets ── */}
          <div key="recent-activity">
            <WidgetShell dragHandleClass={DRAG_HANDLE_CLASS} editing={!gridLocked}>
              <h3 className="mb-3 font-mono text-[12px] font-semibold uppercase tracking-[0.08em] text-[#C9D1D9]">
                Recent Activity
              </h3>
              {recentActivity.length > 0 ? (
                <div className="overflow-hidden rounded-xl border border-[#30363D]">
                  {recentActivity.map((event) => (
                    <ActivityRow
                      key={event.id}
                      event={event}
                      agentMap={agentMap}
                      entityNameMap={entityNameMap}
                      entityTitleMap={entityTitleMap}
                      className={animatedActivityIds.has(event.id) ? "activity-row-enter" : undefined}
                      onOpenSubject={
                        drill
                          ? (entityType, entityId) => {
                              if (entityType === "task") drill({ type: "task", taskId: entityId });
                              else drill({ type: "inbox" });
                            }
                          : undefined
                      }
                    />
                  ))}
                </div>
              ) : (
                <p className="text-sm text-[#999999]">[NO ACTIVITY YET]</p>
              )}
            </WidgetShell>
          </div>

          <div key="recent-tasks">
            <WidgetShell dragHandleClass={DRAG_HANDLE_CLASS} editing={!gridLocked}>
              <h3 className="mb-3 font-mono text-[12px] font-semibold uppercase tracking-[0.08em] text-[#C9D1D9]">
                Recent Tasks
              </h3>
              {recentIssues.length === 0 ? (
                <p className="text-sm text-[#999999]">[NO TASKS YET]</p>
              ) : (
                <div className="divide-y divide-[#30363D] overflow-hidden rounded-xl border border-[#30363D]">
                  {recentIssues.slice(0, 10).map((issue) =>
                    drill ? (
                      <button
                        key={issue.id}
                        type="button"
                        title="Open in Inbox & tasks"
                        onClick={() => drill({ type: "task", taskId: issue.id })}
                        className="block w-full cursor-pointer px-4 py-3 text-left text-sm text-inherit transition-colors duration-200 ease-out hover:bg-[#161b22]"
                      >
                        <div className="flex items-start gap-2 sm:items-center sm:gap-3">
                          <span className="shrink-0 sm:hidden">
                            <StatusIcon status={issue.status} />
                          </span>
                          <span className="flex min-w-0 flex-1 flex-col gap-1 sm:contents">
                            <span className="line-clamp-2 text-sm sm:order-2 sm:flex-1 sm:min-w-0 sm:line-clamp-none sm:truncate">
                              {issue.title}
                            </span>
                            <span className="flex items-center gap-2 sm:order-1 sm:shrink-0">
                              <span className="hidden sm:inline-flex">
                                <StatusIcon status={issue.status} />
                              </span>
                              <span className="font-mono text-xs text-muted-foreground">
                                {issue.identifier ?? issue.id.slice(0, 8)}
                              </span>
                              {issue.assigneeAgentId &&
                                (() => {
                                  const name = agentName(issue.assigneeAgentId);
                                  return name ? (
                                    <span className="hidden sm:inline-flex" title={name}>
                                      <Identity name={name} size="sm" />
                                    </span>
                                  ) : null;
                                })()}
                              <span className="text-xs text-muted-foreground sm:hidden">&middot;</span>
                              <span className="shrink-0 text-xs text-muted-foreground sm:order-last">
                                {timeAgo(issue.updatedAt)}
                              </span>
                            </span>
                          </span>
                        </div>
                      </button>
                    ) : (
                      <a
                        key={issue.id}
                        href={hrefTasks ?? "#"}
                        className="block cursor-pointer px-4 py-3 text-sm no-underline text-inherit transition-colors duration-200 ease-out hover:bg-[#161b22]"
                      >
                        <div className="flex items-start gap-2 sm:items-center sm:gap-3">
                          <span className="shrink-0 sm:hidden">
                            <StatusIcon status={issue.status} />
                          </span>
                          <span className="flex min-w-0 flex-1 flex-col gap-1 sm:contents">
                            <span className="line-clamp-2 text-sm sm:order-2 sm:flex-1 sm:min-w-0 sm:line-clamp-none sm:truncate">
                              {issue.title}
                            </span>
                            <span className="flex items-center gap-2 sm:order-1 sm:shrink-0">
                              <span className="hidden sm:inline-flex">
                                <StatusIcon status={issue.status} />
                              </span>
                              <span className="font-mono text-xs text-muted-foreground">
                                {issue.identifier ?? issue.id.slice(0, 8)}
                              </span>
                              {issue.assigneeAgentId &&
                                (() => {
                                  const name = agentName(issue.assigneeAgentId);
                                  return name ? (
                                    <span className="hidden sm:inline-flex">
                                      <Identity name={name} size="sm" />
                                    </span>
                                  ) : null;
                                })()}
                              <span className="text-xs text-muted-foreground sm:hidden">&middot;</span>
                              <span className="shrink-0 text-xs text-muted-foreground sm:order-last">
                                {timeAgo(issue.updatedAt)}
                              </span>
                            </span>
                          </span>
                        </div>
                      </a>
                    )
                  )}
                </div>
              )}
            </WidgetShell>
          </div>
        </ResponsiveGridLayout>
        </div>
      )}
    </div>
  );
}
