"use client";

import { Suspense, useEffect, useMemo, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { CheckCircle2, Circle, AlertCircle, Clock, Ban, Trash2, UserCheck, ChevronDown } from "lucide-react";
import { Badge } from "@/app/components/ui/badge";
import { Button } from "@/app/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/app/components/ui/card";
import { WorkspaceNewIssueForm } from "@/app/components/console/WorkspaceNewIssueForm";
import { ScrollArea } from "@/app/components/ui/scroll-area";
import { Skeleton } from "@/app/components/ui/skeleton";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { capabilityRefsFromTask, workspaceAttachmentPathsFromTask } from "@/app/components/TaskListPanel";
import { taskToPcIssue } from "@/app/lib/hsm-api-adapter";
import type { HsmCompanyAgentRow, HsmTaskRow } from "@/app/lib/hsm-api-types";
import { useCompanyAgents, useCompanyTasks } from "@/app/lib/hsm-queries";
import {
  buildIssueSpecFromPlan,
  buildIssueTitleFromPlan,
  isDoneTask,
  isPlanTask,
  truncatePath,
} from "@/app/lib/workspace-issue";
import { cn } from "@/app/lib/utils";

/* ─── Helpers ──────────────────────────────────────────────────────────── */

function taskMatchesQuery(task: HsmTaskRow, sp: URLSearchParams): boolean {
  const ids = sp.get("ids");
  if (ids) {
    const want = new Set(ids.split(",").filter(Boolean));
    if (want.size > 0 && !want.has(task.id)) return false;
  }
  const priority = sp.get("priority");
  if (priority !== null && priority !== "") {
    const p = Number(priority);
    if (Number.isFinite(p) && (task.priority ?? 0) !== p) return false;
  }
  const state = sp.get("state");
  if (state && task.state !== state) return false;
  const f = sp.get("filter");
  if (f === "in_progress" && !(/progress|doing|active/i.test(task.state) || !!task.checked_out_by)) return false;
  if (f === "open" && !/open|todo|pending/i.test(task.state)) return false;
  if (f === "blocked" && !/block/i.test(task.state)) return false;
  if (f === "completed" && !/done|complete|closed/i.test(task.state)) return false;
  const view = sp.get("view");
  if ((view === "waiting_admin" || view === "pending_approvals") && task.decision_mode !== "admin_required") return false;
  if (view === "human_inbox") {
    const needs = task.requires_human === true || task.state === "waiting_admin" || task.state === "blocked";
    if (!needs) return false;
  }
  const project = sp.get("project");
  if (project && project !== "" && task.project_id !== project) return false;
  return true;
}

/* ─── Status badge ─────────────────────────────────────────────────────── */

type StatusConfig = {
  label: string;
  icon: React.ReactNode;
  className: string;
  done: boolean;
};

type RunModeView = {
  label: string;
  toneClass: string;
};

function isRunFailed(task: HsmTaskRow): boolean {
  return (task.run?.status ?? "").toLowerCase() === "error";
}

function isRunRunning(task: HsmTaskRow): boolean {
  return (task.run?.status ?? "").toLowerCase() === "running";
}

function isTaskActive(task: HsmTaskRow): boolean {
  if (isRunFailed(task)) return false;
  if (isRunRunning(task)) return true;
  return /progress|doing|active/i.test(task.state);
}

function getRunModeView(task: HsmTaskRow): RunModeView {
  if (isPlanTask(task)) {
    if (isDoneTask(task)) return { label: "plan ready", toneClass: "border-violet-500/40 text-violet-400" };
    return { label: "plan draft", toneClass: "border-violet-500/40 text-violet-400" };
  }
  const run = task.run;
  if (!run) return { label: "task", toneClass: "border-[#333333] text-muted-foreground" };
  const st = (run.status ?? "").toLowerCase();
  if (st === "running") return { label: "running", toneClass: "border-blue-500/40 text-blue-400" };
  if (st === "error") return { label: "run error", toneClass: "border-red-500/40 text-red-400" };
  if (st === "success") {
    return run.tool_calls > 0
      ? { label: "worker", toneClass: "border-emerald-500/40 text-emerald-400" }
      : { label: "llm-only", toneClass: "border-amber-500/40 text-amber-300" };
  }
  return { label: st || "task", toneClass: "border-[#333333] text-muted-foreground" };
}

function getTaskFailureHint(task: HsmTaskRow): string | null {
  const tail = (task.run?.log_tail ?? "").trim();
  if (!tail) return null;
  if (/llm unavailable for agentic execution/i.test(tail)) {
    return "Worker could not reach an LLM provider. Check OpenRouter/Ollama configuration.";
  }
  if (/no llm providers configured/i.test(tail)) {
    return "No LLM provider configured for worker execution.";
  }
  if (/no tool calls were executed/i.test(tail)) {
    return "Worker stopped before executing any real tool calls.";
  }
  if (/no successful non-dispatch tool completions observed/i.test(tail)) {
    return "Worker called tools but none completed successfully.";
  }
  if (/agentic tool loop ended without a final answer/i.test(tail)) {
    return "Worker hit max turns before producing a final answer.";
  }
  if (/no space left on device|os error 28/i.test(tail)) {
    return "Disk full during execution.";
  }
  return tail.length > 180 ? `${tail.slice(0, 180)}…` : tail;
}

function getStatusConfig(state: string): StatusConfig {
  if (/done|complete|closed/i.test(state))
    return {
      label: "Done",
      icon: <CheckCircle2 className="size-3" />,
      className: "border-emerald-500/40 bg-emerald-500/10 text-emerald-400",
      done: true,
    };
  if (/progress|doing|active/i.test(state))
    return {
      label: "Active",
      icon: <span className="relative flex size-2"><span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-blue-400 opacity-60" /><span className="relative inline-flex size-2 rounded-full bg-blue-400" /></span>,
      className: "border-blue-500/40 bg-blue-500/10 text-blue-400",
      done: false,
    };
  if (/block/i.test(state))
    return {
      label: "Blocked",
      icon: <Ban className="size-3" />,
      className: "border-red-500/40 bg-red-500/10 text-red-400",
      done: false,
    };
  if (/wait.*admin|needs.*human|requires.*human/i.test(state))
    return {
      label: "Needs you",
      icon: <AlertCircle className="size-3" />,
      className: "border-amber-500/40 bg-amber-500/10 text-amber-400",
      done: false,
    };
  if (/in_progress/.test(state))
    return {
      label: "In progress",
      icon: <Clock className="size-3" />,
      className: "border-blue-500/40 bg-blue-500/10 text-blue-400",
      done: false,
    };
  return {
    label: state,
    icon: <Circle className="size-3" />,
    className: "border-admin-border bg-admin-surface text-muted-foreground",
    done: false,
  };
}

/* ─── Reassign popover ─────────────────────────────────────────────────── */

function ReassignMenu({
  taskId,
  agents,
  apiBase,
  onDone,
}: {
  taskId: string;
  agents: HsmCompanyAgentRow[];
  apiBase: string;
  onDone: () => void;
}) {
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  const assign = async (agent: HsmCompanyAgentRow) => {
    setBusy(true);
    try {
      const res = await fetch(`${apiBase}/api/company/tasks/${taskId}/checkout`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ agent_ref: agent.name }),
      });
      if (res.ok) {
        setOpen(false);
        onDone();
        router.push(`/workspace/agents/${agent.id}?tab=workspace`);
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="relative">
      <button
        type="button"
        disabled={busy}
        onClick={() => setOpen((v) => !v)}
        className="inline-flex items-center gap-1 rounded border border-[#2a2a2a] bg-[#111] px-2 py-1 font-mono text-[10px] text-[#888] transition-colors hover:border-[#4a9eff]/50 hover:text-[#4a9eff] disabled:opacity-40"
      >
        <UserCheck className="size-3" />
        Reassign
        <ChevronDown className="size-2.5" />
      </button>
      {open && (
        <>
          <div className="fixed inset-0 z-10" onClick={() => setOpen(false)} />
          <div className="absolute left-0 top-full z-20 mt-1 min-w-[160px] rounded border border-[#2a2a2a] bg-[#0a0a0a] py-1 shadow-xl">
            {agents.map((a) => (
              <button
                key={a.id}
                type="button"
                disabled={busy}
                onClick={() => void assign(a)}
                className="block w-full px-3 py-1.5 text-left font-mono text-[11px] text-[#c8c8c8] hover:bg-white/[0.05] hover:text-[#4a9eff]"
              >
                <span className="block">{a.name}</span>
                {a.title && <span className="block text-[9px] text-[#555]">{a.title}</span>}
              </button>
            ))}
            {agents.length === 0 && (
              <p className="px-3 py-2 font-mono text-[10px] text-[#555]">No agents</p>
            )}
          </div>
        </>
      )}
    </div>
  );
}

/* ─── Delete confirm ───────────────────────────────────────────────────── */

function DeleteButton({
  taskId,
  companyId,
  apiBase,
  onDone,
}: {
  taskId: string;
  companyId: string;
  apiBase: string;
  onDone: () => void;
}) {
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);

  const del = async () => {
    setBusy(true);
    try {
      const res = await fetch(`${apiBase}/api/company/companies/${companyId}/tasks/${taskId}`, { method: "DELETE" });
      if (res.ok) onDone();
    } finally {
      setBusy(false);
      setConfirm(false);
    }
  };

  if (confirm) {
    return (
      <span className="inline-flex items-center gap-1">
        <button
          type="button"
          disabled={busy}
          onClick={() => void del()}
          className="rounded border border-red-500/50 bg-red-500/10 px-2 py-1 font-mono text-[10px] text-red-400 hover:bg-red-500/20 disabled:opacity-40"
        >
          {busy ? "…" : "Confirm"}
        </button>
        <button
          type="button"
          onClick={() => setConfirm(false)}
          className="rounded border border-[#2a2a2a] px-2 py-1 font-mono text-[10px] text-[#666] hover:text-[#999]"
        >
          Cancel
        </button>
      </span>
    );
  }

  return (
    <button
      type="button"
      onClick={() => setConfirm(true)}
      className="inline-flex items-center gap-1 rounded border border-[#2a2a2a] bg-[#111] px-2 py-1 font-mono text-[10px] text-[#888] transition-colors hover:border-red-500/50 hover:text-red-400"
    >
      <Trash2 className="size-3" />
      Delete
    </button>
  );
}

/* ─── Task row ─────────────────────────────────────────────────────────── */

function TaskRow({
  issue,
  task,
  agents,
  apiBase,
  companyId,
  onRefresh,
  onSelect,
  onBuildPlan,
  buildPending,
}: {
  issue: ReturnType<typeof taskToPcIssue>;
  task: HsmTaskRow;
  agents: HsmCompanyAgentRow[];
  apiBase: string;
  companyId: string;
  onRefresh: () => void;
  onSelect: () => void;
  onBuildPlan: () => void;
  buildPending: boolean;
}) {
  const [hovered, setHovered] = useState(false);
  const [markingDone, setMarkingDone] = useState(false);
  const caps = capabilityRefsFromTask(task);
  const wsPaths = workspaceAttachmentPathsFromTask(task);
  const runMode = getRunModeView(task);
  const failureHint = getTaskFailureHint(task);
  const statusCfg = isRunFailed(task) && !/done|complete|closed/i.test(task.state)
    ? { label: "Run failed", icon: <AlertCircle className="size-3" />, className: "border-red-500/40 bg-red-500/10 text-red-400", done: false }
    : task.requires_human && !/done|complete|closed/i.test(task.state)
      ? getStatusConfig("waiting_admin")
      : getStatusConfig(task.state);
  const isDone = statusCfg.done;

  const markDone = async () => {
    setMarkingDone(true);
    try {
      const res = await fetch(`${apiBase}/api/company/tasks/${task.id}/state`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ state: "done" }),
      });
      if (res.ok) onRefresh();
    } finally {
      setMarkingDone(false);
    }
  };

  return (
    <div
      className={cn(
        "group border-b border-admin-border last:border-b-0 transition-colors",
        isDone ? "opacity-60 hover:opacity-100" : "",
        hovered ? "bg-white/[0.02]" : "",
      )}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      {wsPaths.length > 0 ? (
        <div className="border-b border-amber-400/20 bg-amber-400/10 px-4 py-1 text-[11px] text-amber-100">
          <span className="font-medium">Workspace file</span>
          <span className="ml-2 font-mono text-[10px] opacity-80">
            {wsPaths.length === 1 ? truncatePath(wsPaths[0]!) : `${wsPaths.length} paths · ${truncatePath(wsPaths[0]!)}`}
          </span>
        </div>
      ) : null}

      <div
        data-task-row={issue.id}
        className="grid w-full items-center grid-cols-[100px_1fr_40px_148px_88px_72px_110px_auto] gap-2 px-4 py-3"
      >
        {/* Key */}
        <button
          type="button"
          className="font-mono text-xs text-primary text-left hover:underline"
          onClick={onSelect}
        >
          {issue.identifier}
        </button>

        {/* Title */}
        <button type="button" className="min-w-0 text-left" onClick={onSelect}>
          <span className={cn("block truncate font-medium text-foreground text-sm", isDone && "line-through text-muted-foreground")}>
            {issue.title}
          </span>
          {failureHint ? (
            <span className="mt-0.5 block truncate font-mono text-[10px] text-amber-300" title={task.run?.log_tail ?? undefined}>
              Why it failed: {failureHint}
            </span>
          ) : null}
          {caps.filter((c) => c.kind !== "mode").length > 0 ? (
            <span className="mt-0.5 flex flex-wrap gap-1">
              {caps.filter((c) => c.kind !== "mode").slice(0, 3).map((c) => (
                <Badge
                  key={`${issue.id}-${c.kind}-${c.ref}`}
                  variant="outline"
                  className="border-emerald-500/40 font-mono text-[9px] text-emerald-400"
                >
                  {c.ref.length > 18 ? `${c.ref.slice(0, 16)}…` : c.ref}
                </Badge>
              ))}
              {caps.filter((c) => c.kind !== "mode").length > 3 && (
                <span className="self-center font-mono text-[9px] text-muted-foreground">
                  +{caps.filter((c) => c.kind !== "mode").length - 3}
                </span>
              )}
            </span>
          ) : null}
        </button>

        {/* Priority */}
        <span className="font-mono text-xs text-muted-foreground">{issue.priority ?? "—"}</span>

        {/* Status */}
        <span>
          <span className={cn("inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 font-mono text-[10px] font-medium", statusCfg.className)}>
            {statusCfg.icon}
            {statusCfg.label}
          </span>
        </span>

        {/* Gate */}
        <span>
          {task.requires_human || issue.decisionMode === "admin_required" ? (
            <Badge variant="outline" className="border-amber-500/50 font-mono text-[9px] text-amber-400">
              needs human
            </Badge>
          ) : (
            <span className="text-xs text-muted-foreground">—</span>
          )}
        </span>

        {/* Mode */}
        <span>
          {task && isPlanTask(task) ? (
            isDoneTask(task) ? (
              <Button
                variant="outline"
                size="xs"
                className="border-violet-500/50 font-mono text-[9px] text-violet-400 hover:bg-violet-500/10"
                disabled={buildPending}
                onClick={onBuildPlan}
              >
                Build
              </Button>
            ) : (
              <Badge variant="outline" className="border-violet-500/40 font-mono text-[9px] text-violet-400">
                {runMode.label}
              </Badge>
            )
          ) : (
            <Badge variant="outline" className={cn("font-mono text-[9px]", runMode.toneClass)}>
              {runMode.label}
            </Badge>
          )}
        </span>

        {/* Assignee */}
        <span className="truncate font-mono text-xs text-muted-foreground">
          {task.checked_out_by ?? task.owner_persona ?? "—"}
        </span>

        {/* Row actions — visible on hover */}
        <div className={cn("flex items-center gap-1.5 transition-opacity", hovered ? "opacity-100" : "opacity-0 pointer-events-none")}>
          {!isDone && (
            <button
              type="button"
              disabled={markingDone}
              onClick={() => void markDone()}
              className="inline-flex items-center gap-1 rounded border border-[#2a2a2a] bg-[#111] px-2 py-1 font-mono text-[10px] text-[#888] transition-colors hover:border-emerald-500/50 hover:text-emerald-400 disabled:opacity-40"
            >
              <CheckCircle2 className="size-3" />
              Done
            </button>
          )}
          <ReassignMenu taskId={task.id} agents={agents} apiBase={apiBase} onDone={onRefresh} />
          <DeleteButton taskId={task.id} companyId={companyId} apiBase={apiBase} onDone={onRefresh} />
        </div>
      </div>
    </div>
  );
}

/* ─── Page ─────────────────────────────────────────────────────────────── */

function IssuesContent() {
  const searchParams = useSearchParams();
  const qc = useQueryClient();
  const { apiBase, companyId, companies, setPropertiesSelection, propertiesSelection } = useWorkspace();
  const company = companies.find((c) => c.id === companyId);
  const prefix = (company?.issue_key_prefix ?? "HSM").toUpperCase();

  const { data: tasks = [], isLoading, error } = useCompanyTasks(apiBase, companyId);
  const { data: agentsRaw = [] } = useCompanyAgents(apiBase, companyId);

  const activeAgents = useMemo(
    () =>
      agentsRaw
        .filter((a) => (a.status ?? "").toLowerCase() !== "terminated" && a.name.trim())
        .sort((a, b) => a.name.localeCompare(b.name)),
    [agentsRaw],
  );

  const firstAgent = agentsRaw.filter((a) => (a.status ?? "").toLowerCase() !== "terminated").sort((a, b) => a.name.localeCompare(b.name))[0];
  const assigneePersona = firstAgent?.name ?? "";
  const assigneeDisplayName = firstAgent?.title ?? firstAgent?.name ?? company?.display_name ?? "Agent";

  const createTask = useMutation({
    mutationFn: async (overrides: { title: string; specification?: string | null; parentTaskId?: string }) => {
      const title = overrides.title.trim();
      if (!title) throw new Error("Title is required.");
      const body: Record<string, unknown> = { title, specification: overrides.specification ?? null };
      if (overrides.parentTaskId) body.parent_task_id = overrides.parentTaskId;
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/tasks`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["hsm", "tasks", apiBase, companyId] });
      void qc.invalidateQueries({ queryKey: ["hsm", "intelligence", apiBase, companyId] });
    },
  });

  const refresh = () => {
    void qc.invalidateQueries({ queryKey: ["hsm", "tasks", apiBase, companyId] });
  };

  const filtered = useMemo(() => {
    const sp = new URLSearchParams(searchParams.toString());
    const hasFilters = sp.has("ids") || sp.has("priority") || sp.has("state") || sp.has("filter") || sp.has("view") || sp.has("project");
    if (!hasFilters) return tasks;
    return tasks.filter((t) => taskMatchesQuery(t, sp));
  }, [tasks, searchParams]);

  const issues = useMemo(() => filtered.map((t) => taskToPcIssue(t, prefix)), [filtered, prefix]);

  useEffect(() => {
    const id = searchParams.get("focus");
    if (!id || tasks.length === 0) return;
    // `tasks` refetches often (e.g. after operator agent-chat). Do not replace an active right-rail
    // **agent** session with this deep-link — that was kicking users back to task header + roster mid-chat.
    if (propertiesSelection?.kind === "agent") return;
    const t = tasks.find((x) => x.id === id);
    if (!t) return;
    const pc = taskToPcIssue(t, prefix);
    setPropertiesSelection({ kind: "task", id: t.id, title: `${pc.identifier} · ${t.title}` });
    requestAnimationFrame(() => {
      document.querySelector(`[data-task-row="${id}"]`)?.scrollIntoView({ block: "nearest", behavior: "smooth" });
    });
  }, [searchParams, tasks, prefix, setPropertiesSelection, propertiesSelection]);

  if (!companyId) return <p className="pc-page-desc">Select a company in the header.</p>;
  if (error) return (
    <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-4 text-sm">
      {error instanceof Error ? error.message : String(error)}
    </div>
  );

  const filterHint = searchParams.toString() ? (
    <span className="mt-1 block font-mono text-[10px] text-muted-foreground">Filtered: {searchParams.toString()}</span>
  ) : null;

  const doneCount = issues.filter((_, i) => getStatusConfig(filtered[i]?.state ?? "").done).length;
  const activeCount = issues.filter((_, i) => {
    const task = filtered[i];
    return task ? isTaskActive(task) : false;
  }).length;
  const failedCount = issues.filter((_, i) => {
    const task = filtered[i];
    return task ? isRunFailed(task) : false;
  }).length;

  return (
    <div className="space-y-4">
      <div>
        <p className="pc-page-eyebrow">Company OS</p>
        <h1 className="pc-page-title">Issues</h1>
        <p className="pc-page-desc">
          Open work for this company — each row is a task. Hover any row to mark done, reassign, or delete.
        </p>
        {filterHint}
      </div>

      <Card className="pc-panel border-admin-border">
        <CardHeader className="pb-2">
          <CardTitle className="text-base">New issue</CardTitle>
          <CardDescription>
            Create a task: plan vs task, project, priority, labels, who reviews, optional repeat cadence, and file paths.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <WorkspaceNewIssueForm
            apiBase={apiBase}
            companyId={companyId}
            assigneeDisplayName={assigneeDisplayName}
            assigneePersona={assigneePersona}
            idPrefix="issues-issue"
            showAttachBanner={false}
          />
        </CardContent>
      </Card>

      <Card className="pc-panel border-admin-border">
        <CardHeader className="pb-2">
          <div className="flex items-center justify-between gap-4">
            <div>
              <CardTitle className="text-base">Task list</CardTitle>
              <CardDescription>
                {isLoading ? "Loading…" : (
                  <span className="inline-flex flex-wrap gap-3">
                    <span>{issues.length} issues</span>
                    {activeCount > 0 && (
                      <span className="inline-flex items-center gap-1 text-blue-400">
                        <span className="relative flex size-1.5"><span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-blue-400 opacity-60" /><span className="relative inline-flex size-1.5 rounded-full bg-blue-400" /></span>
                        {activeCount} active
                      </span>
                    )}
                    {doneCount > 0 && (
                      <span className="inline-flex items-center gap-1 text-emerald-400">
                        <CheckCircle2 className="size-3" />
                        {doneCount} done
                      </span>
                    )}
                    {failedCount > 0 && (
                      <span className="inline-flex items-center gap-1 text-red-400">
                        <AlertCircle className="size-3" />
                        {failedCount} failed
                      </span>
                    )}
                    <span className="text-muted-foreground">
                      Gate = approvals/human action. Mode = plan/worker/llm state.
                    </span>
                  </span>
                )}
              </CardDescription>
            </div>
          </div>
        </CardHeader>
        <CardContent className="p-0">
          <ScrollArea className="h-[min(70vh,600px)]">
            <div className="min-w-[900px]">
              {/* Table header */}
              <div className="grid grid-cols-[100px_1fr_40px_148px_88px_72px_110px_auto] gap-2 border-b border-admin-border px-4 py-2 font-mono text-[10px] uppercase tracking-widest text-muted-foreground">
                <span>Key</span>
                <span>Title</span>
                <span>Priority</span>
                <span>Status</span>
                <span>Gate</span>
                <span>Mode</span>
                <span>Assignee</span>
                <span />
              </div>

              {isLoading
                ? Array.from({ length: 6 }).map((_, i) => (
                    <div key={i} className="flex gap-3 border-b border-admin-border px-4 py-3">
                      <Skeleton className="h-4 w-20" />
                      <Skeleton className="h-4 flex-1" />
                      <Skeleton className="h-4 w-24" />
                    </div>
                  ))
                : issues.length === 0
                  ? (
                    <div className="px-4 py-12 text-center text-sm text-muted-foreground">
                      No tasks match this filter.
                    </div>
                  )
                  : issues.map((issue, idx) => {
                      const task = filtered[idx]!;
                      return (
                        <TaskRow
                          key={issue.id}
                          issue={issue}
                          task={task}
                          agents={activeAgents}
                          apiBase={apiBase}
                          companyId={companyId!}
                          onRefresh={refresh}
                          onSelect={() =>
                            setPropertiesSelection({
                              kind: "task",
                              id: issue.id,
                              title: `${issue.identifier} · ${issue.title}`,
                            })
                          }
                          onBuildPlan={() =>
                            createTask.mutate({
                              title: buildIssueTitleFromPlan(task.title),
                              specification: buildIssueSpecFromPlan(task.specification),
                              parentTaskId: task.id,
                            })
                          }
                          buildPending={createTask.isPending}
                        />
                      );
                    })}
            </div>
          </ScrollArea>
        </CardContent>
      </Card>
    </div>
  );
}

function IssuesFallback() {
  return (
    <div className="space-y-4">
      <Skeleton className="h-8 w-48" />
      <Skeleton className="h-64 w-full rounded-lg" />
    </div>
  );
}

export default function WorkspaceIssuesPage() {
  return (
    <Suspense fallback={<IssuesFallback />}>
      <IssuesContent />
    </Suspense>
  );
}
