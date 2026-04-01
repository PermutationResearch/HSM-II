"use client";

/**
 * Layout and IA ported from Paperclip’s open-source dashboard:
 * https://github.com/paperclipai/paperclip/blob/master/ui/src/pages/Dashboard.tsx
 *
 * Data comes from HSM `hsm_console` (Company OS). We do not ship Paperclip’s proprietary backend;
 * the UI is adapted so metrics, agents, issues, and activity map to our APIs.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import {
  Bot,
  CircleDot,
  DollarSign,
  LayoutDashboard,
  PauseCircle,
  ShieldCheck,
} from "lucide-react";
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
  /**
   * `nothing` — typography hero + monochrome charts + ambient gradient shell.
   * `admin` — Paperclip-style dense console (colored run activity, GitHub-adjacent surfaces).
   */
  layout?: "nothing" | "admin";
  /** Navigate to Inbox & tasks with filters / scroll (Command dashboard drill-down). */
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

  const [animatedActivityIds, setAnimatedActivityIds] = useState<Set<string>>(new Set());
  const seenActivityIdsRef = useRef<Set<string>>(new Set());
  const hydratedActivityRef = useRef(false);
  const activityAnimationTimersRef = useRef<number[]>([]);

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

  const hasNoAgents = agents.length === 0 && (data?.tasks.length ?? 0) === 0;

  const dm = dashboardMetrics;
  const runs = data?.tasks ?? [];
  const companyLabel = companies.find((c) => c.id === companyId)?.display_name ?? null;

  const drill = onDrillDown;
  const metricTo = (fallback?: string) => (drill ? undefined : fallback);
  const metricClick = (fn: () => void) => (drill ? fn : undefined);

  const shellClass = isAdmin
    ? "-mx-6 space-y-6 bg-[#010409] px-6 pb-10"
    : "nd-dashboard-shell -mx-6 space-y-8 px-6 pb-10";

  return (
    <div className={shellClass}>
      {isAdmin ? (
        <header className="flex flex-col gap-3 border-b border-[#30363D] pb-6 sm:flex-row sm:items-start sm:justify-between sm:gap-4">
          <div className="min-w-0">
            <h1 className="text-lg font-medium tracking-tight text-white">Dashboard</h1>
            <p className="mt-1 text-sm text-[#8B949E]">
              {companyLabel ?? "Company workspace"} · multi-agent overview
            </p>
          </div>
          {dm.agents.running > 0 ? (
            <span className="inline-flex w-fit shrink-0 items-center rounded-md border border-[#388bfd]/40 bg-[#388bfd]/10 px-2.5 py-1 font-mono text-[11px] font-semibold uppercase tracking-wide text-[#58a6ff]">
              {dm.agents.running} live
            </span>
          ) : (
            <span className="font-mono text-[11px] uppercase tracking-wide text-[#484f58]">No live runs</span>
          )}
        </header>
      ) : (
        <header className="border-b border-border pb-8">
          <p className="nd-label">Dashboard</p>
          <div className="mt-6 flex flex-col gap-2 sm:flex-row sm:items-end sm:justify-between sm:gap-8">
            <div className="min-w-0">
              <p className="font-mono text-[clamp(2.75rem,7vw,3.75rem)] font-normal tabular-nums leading-[1.05] tracking-tight text-white">
                {dm.totalEnabled}
              </p>
              <p className="mt-3 max-w-xl text-sm font-normal leading-relaxed text-muted-foreground">
                Agents enabled
                {companyLabel ? (
                  <>
                    {" "}
                    · <span className="text-foreground">{companyLabel}</span>
                  </>
                ) : null}
              </p>
            </div>
            <p className="nd-label shrink-0 text-[#666666]">Live snapshot</p>
          </div>
        </header>
      )}

      {error && <p className="text-sm text-destructive">{error}</p>}

      {hasNoAgents && (
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

      {data && (
        <>
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

          <div className="grid grid-cols-2 gap-1 sm:gap-2 xl:grid-cols-4">
            <MetricCard
              icon={Bot}
              value={dm.totalEnabled}
              label="Agents Enabled"
              variant={isAdmin ? "admin" : "nothing"}
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
            <MetricCard
              icon={CircleDot}
              value={dm.tasks.inProgress}
              label="Tasks In Progress"
              variant={isAdmin ? "admin" : "nothing"}
              to={metricTo(hrefTasks)}
              onClick={metricClick(() => drill?.({ type: "filter_in_progress" }))}
              description={
                <span>
                  {dm.tasks.open} open{", "}
                  {dm.tasks.blocked} blocked
                </span>
              }
            />
            <MetricCard
              icon={DollarSign}
              value={formatCents(dm.costs.monthSpendCents)}
              label="Month Spend"
              variant={isAdmin ? "admin" : "nothing"}
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
            <MetricCard
              icon={ShieldCheck}
              value={dm.pendingApprovals + dm.budgets.pendingApprovals}
              label="Pending Approvals"
              variant={isAdmin ? "admin" : "nothing"}
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
          </div>

          <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
            <ChartCard title="Run Activity" subtitle="Last 14 days" layout={isAdmin ? "admin" : "nothing"}>
              {drill ? (
                <p className="mb-2 font-mono text-[10px] uppercase tracking-wide text-[#484f58]">
                  Bar → tasks in that day bucket
                </p>
              ) : null}
              <RunActivityChart
                tasks={runs}
                variant={isAdmin ? "admin" : "nothing"}
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
            <ChartCard title="Issues by Priority" subtitle="Last 14 days" layout={isAdmin ? "admin" : "nothing"}>
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
            <ChartCard title="Issues by Status" subtitle="Last 14 days" layout={isAdmin ? "admin" : "nothing"}>
              {drill ? (
                <p className="mb-2 font-mono text-[10px] uppercase tracking-wide text-[#484f58]">
                  Bar → tasks in that status
                </p>
              ) : null}
              <IssueStatusChart tasks={runs} onStatusClick={drill ? (state) => drill({ type: "filter_state", state }) : undefined} />
            </ChartCard>
            <ChartCard title="Success Rate" subtitle="Last 14 days" layout={isAdmin ? "admin" : "nothing"}>
              {drill ? (
                <p className="mb-2 font-mono text-[10px] uppercase tracking-wide text-[#484f58]">
                  Bar → completed tasks
                </p>
              ) : null}
              <SuccessRateChart tasks={runs} onCompletedClick={drill ? () => drill({ type: "filter_completed" }) : undefined} />
            </ChartCard>
          </div>

          <div className="grid gap-4 md:grid-cols-2">
            {recentActivity.length > 0 && (
              <div className="min-w-0">
                <h3 className={isAdmin ? "mb-3 font-mono text-[12px] font-semibold uppercase tracking-[0.08em] text-[#C9D1D9]" : "nd-label mb-3"}>
                  Recent Activity
                </h3>
                <div className={isAdmin ? "overflow-hidden rounded-2xl border border-[#30363D]" : "overflow-hidden rounded-2xl border border-border"}>
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
              </div>
            )}

            <div className="min-w-0">
              <h3 className={isAdmin ? "mb-3 font-mono text-[12px] font-semibold uppercase tracking-[0.08em] text-[#C9D1D9]" : "nd-label mb-3"}>
                Recent Tasks
              </h3>
              {recentIssues.length === 0 ? (
                <div className={isAdmin ? "border border-[#30363D] p-4" : "border border-border p-4"}>
                  <p className="text-sm text-[#999999]">[NO TASKS YET]</p>
                </div>
              ) : (
                <div
                  className={
                    isAdmin
                      ? "divide-y divide-[#30363D] overflow-hidden rounded-2xl border border-[#30363D]"
                      : "divide-y divide-border overflow-hidden rounded-2xl border border-border"
                  }
                >
                  {recentIssues.slice(0, 10).map((issue) =>
                    drill ? (
                      <button
                        key={issue.id}
                        type="button"
                        title="Open in Inbox & tasks"
                        onClick={() => drill({ type: "task", taskId: issue.id })}
                        className={
                          isAdmin
                            ? "block w-full cursor-pointer px-4 py-3 text-left text-sm text-inherit transition-colors duration-200 ease-out hover:bg-[#161b22]"
                            : "block w-full cursor-pointer px-4 py-3 text-left text-sm text-inherit transition-colors duration-200 ease-out hover:bg-[#1A1A1A]"
                        }
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
                        className={
                          isAdmin
                            ? "block cursor-pointer px-4 py-3 text-sm no-underline text-inherit transition-colors duration-200 ease-out hover:bg-[#161b22]"
                            : "block cursor-pointer px-4 py-3 text-sm no-underline text-inherit transition-colors duration-200 ease-out hover:bg-[#1A1A1A]"
                        }
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
            </div>
          </div>
        </>
      )}
    </div>
  );
}
