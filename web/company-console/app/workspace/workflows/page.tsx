"use client";

import { useMemo, useState } from "react";
import { useRouter } from "next/navigation";
import { Archive, CheckCircle2, Compass, GitBranch, Play, Pencil, Circle, AlertCircle } from "lucide-react";
import { Badge } from "@/app/components/ui/badge";
import { Button } from "@/app/components/ui/button";
import { Skeleton } from "@/app/components/ui/skeleton";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { capabilityRefsFromTask } from "@/app/components/TaskListPanel";
import { useCompanyTasks } from "@/app/lib/hsm-queries";
import { isDoneTask } from "@/app/lib/workspace-issue";
import type { HsmTaskRow } from "@/app/lib/hsm-api-types";
import { cn } from "@/app/lib/utils";

/* ─── Helpers ──────────────────────────────────────────────────────────── */

type WorkflowKind = "explore_plan_build" | "workflow" | "explore" | "plan";

type WorkflowTask = HsmTaskRow & {
  workflowKind: WorkflowKind;
  stages: StageInfo[];
  variableCount: number;
};

type StageInfo = {
  label: string;
  color: string;
};

const STAGE_CHIPS: Record<WorkflowKind, StageInfo[]> = {
  explore_plan_build: [
    { label: "Explore", color: "bg-violet-500/20 text-violet-300 border-violet-500/30" },
    { label: "Plan", color: "bg-sky-500/20 text-sky-300 border-sky-500/30" },
    { label: "Build", color: "bg-emerald-500/20 text-emerald-300 border-emerald-500/30" },
  ],
  workflow: [
    { label: "Multi-stage", color: "bg-amber-500/20 text-amber-300 border-amber-500/30" },
  ],
  explore: [
    { label: "Explore", color: "bg-violet-500/20 text-violet-300 border-violet-500/30" },
  ],
  plan: [
    { label: "Plan", color: "bg-sky-500/20 text-sky-300 border-sky-500/30" },
    { label: "Build", color: "bg-emerald-500/20 text-emerald-300 border-emerald-500/30" },
  ],
};

function detectWorkflowKind(task: HsmTaskRow): WorkflowKind | null {
  const refs = capabilityRefsFromTask(task);
  if (refs.some((c) => c.kind === "mode" && c.ref === "pipeline:explore_plan_build")) {
    return "explore_plan_build";
  }
  if (refs.some((c) => c.kind === "mode" && c.ref === "pipeline:workflow")) {
    return "workflow";
  }
  if (refs.some((c) => c.kind === "mode" && c.ref === "explore")) {
    return "explore";
  }
  if (refs.some((c) => c.kind === "mode" && c.ref === "plan")) {
    return "plan";
  }
  return null;
}

function countVariables(task: HsmTaskRow): number {
  // Count capability_refs beyond the mode refs as "variables"
  const refs = capabilityRefsFromTask(task);
  return refs.filter((c) => c.kind !== "mode").length;
}

function toWorkflowTask(task: HsmTaskRow): WorkflowTask | null {
  const kind = detectWorkflowKind(task);
  if (!kind) return null;
  return {
    ...task,
    workflowKind: kind,
    stages: STAGE_CHIPS[kind],
    variableCount: countVariables(task),
  };
}

/* ─── Status badge ─────────────────────────────────────────────────────── */

function WorkflowStatusBadge({ task }: { task: HsmTaskRow }) {
  const done = isDoneTask(task);
  const active =
    !done && (task.checked_out_by || /progress|doing|active/i.test(task.state));
  const blocked = !done && /block/i.test(task.state);

  if (done) {
    return (
      <span className="flex items-center gap-1 font-mono text-[9px] uppercase tracking-wide text-emerald-400">
        <CheckCircle2 className="size-2.5" />
        Done
      </span>
    );
  }
  if (blocked) {
    return (
      <span className="flex items-center gap-1 font-mono text-[9px] uppercase tracking-wide text-amber-400">
        <AlertCircle className="size-2.5" />
        Blocked
      </span>
    );
  }
  if (active) {
    return (
      <span className="flex items-center gap-1 font-mono text-[9px] uppercase tracking-wide text-blue-400">
        <span className="relative flex size-2">
          <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-blue-400 opacity-60" />
          <span className="relative inline-flex size-2 rounded-full bg-blue-400" />
        </span>
        Running
      </span>
    );
  }
  return (
    <span className="flex items-center gap-1 font-mono text-[9px] uppercase tracking-wide text-[#555555]">
      <Circle className="size-2.5" />
      Open
    </span>
  );
}

/* ─── Workflow card ─────────────────────────────────────────────────────── */

function WorkflowCard({
  task,
  apiBase,
  companyId,
  onRefresh,
}: {
  task: WorkflowTask;
  apiBase: string;
  companyId: string;
  onRefresh: () => void;
}) {
  const router = useRouter();
  const [archiving, setArchiving] = useState(false);

  const done = isDoneTask(task);

  async function handleArchive() {
    if (!confirm(`Archive workflow "${task.title}"?`)) return;
    setArchiving(true);
    try {
      await fetch(
        `${apiBase}/api/company/companies/${companyId}/tasks/${task.id}`,
        {
          method: "PATCH",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ state: "cancelled" }),
        },
      );
      onRefresh();
    } finally {
      setArchiving(false);
    }
  }

  function handleEdit() {
    router.push(`/workspace/issues?ids=${task.id}`);
  }

  function handleRun() {
    router.push(`/workspace/issues?ids=${task.id}`);
  }

  return (
    <div
      className={cn(
        "flex flex-col gap-3 rounded-xl border bg-[#080808] p-4 transition-colors",
        done ? "border-[#1a1a1a] opacity-60" : "border-[#1e1e1e] hover:border-[#2a2a2a]",
      )}
    >
      {/* Header row */}
      <div className="flex items-start gap-3">
        <div className="mt-0.5 rounded-md border border-[#1a1a1a] bg-[#0e0e0e] p-1.5">
          {task.workflowKind === "explore_plan_build" || task.workflowKind === "explore" ? (
            <Compass className="size-4 text-violet-400" strokeWidth={1.5} />
          ) : (
            <GitBranch className="size-4 text-amber-400" strokeWidth={1.5} />
          )}
        </div>
        <div className="flex-1 min-w-0">
          <p className="text-[13px] font-medium text-[#d4d4d4] leading-snug line-clamp-2">
            {task.title}
          </p>
          {task.owner_persona ? (
            <p className="mt-0.5 font-mono text-[10px] text-[#444444]">
              {task.owner_persona}
            </p>
          ) : null}
        </div>
        <WorkflowStatusBadge task={task} />
      </div>

      {/* Stage chips */}
      <div className="flex flex-wrap items-center gap-1.5">
        {task.stages.map((stage, i) => (
          <>
            <span
              key={stage.label}
              className={cn(
                "inline-flex items-center rounded border px-2 py-0.5 font-mono text-[9px] font-medium uppercase tracking-wide",
                stage.color,
              )}
            >
              {stage.label}
            </span>
            {i < task.stages.length - 1 ? (
              <span key={`arrow-${i}`} className="font-mono text-[9px] text-[#333333]">→</span>
            ) : null}
          </>
        ))}
        {task.variableCount > 0 ? (
          <span className="ml-1 font-mono text-[9px] text-[#3a3a3a]">
            {task.variableCount} var{task.variableCount !== 1 ? "s" : ""}
          </span>
        ) : null}
      </div>

      {/* Action row */}
      <div className="flex items-center gap-2 pt-1 border-t border-[#111111]">
        {!done ? (
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="gap-1.5 border-[#2a2a2a] bg-black px-3 py-1.5 font-mono text-[10px] uppercase tracking-wide text-[#e8e8e8] hover:bg-white/[0.06]"
            onClick={handleRun}
          >
            <Play className="size-3" strokeWidth={1.5} />
            Run
          </Button>
        ) : null}
        <Button
          type="button"
          size="sm"
          variant="ghost"
          className="gap-1.5 px-2 py-1.5 font-mono text-[10px] uppercase tracking-wide text-[#555555] hover:text-[#e8e8e8]"
          onClick={handleEdit}
        >
          <Pencil className="size-3" strokeWidth={1.5} />
          Edit
        </Button>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          disabled={archiving}
          className="ml-auto gap-1.5 px-2 py-1.5 font-mono text-[10px] uppercase tracking-wide text-[#333333] hover:text-[#888888]"
          onClick={() => void handleArchive()}
        >
          <Archive className="size-3" strokeWidth={1.5} />
          Archive
        </Button>
      </div>
    </div>
  );
}

/* ─── Page ─────────────────────────────────────────────────────────────── */

const FILTER_OPTIONS = [
  { value: "all", label: "All" },
  { value: "active", label: "Active" },
  { value: "done", label: "Done" },
] as const;

type FilterValue = (typeof FILTER_OPTIONS)[number]["value"];

export default function WorkflowsPage() {
  const { apiBase, companyId } = useWorkspace();
  const { data: tasksRaw = [], isLoading, refetch } = useCompanyTasks(apiBase, companyId);
  const [filter, setFilter] = useState<FilterValue>("active");

  const workflows = useMemo<WorkflowTask[]>(() => {
    return tasksRaw
      .map(toWorkflowTask)
      .filter((t): t is WorkflowTask => t !== null)
      .sort((a, b) => (b.priority ?? 0) - (a.priority ?? 0));
  }, [tasksRaw]);

  const filtered = useMemo(() => {
    if (filter === "active") return workflows.filter((t) => !isDoneTask(t) && !/cancel/i.test(t.state));
    if (filter === "done") return workflows.filter((t) => isDoneTask(t) || /cancel/i.test(t.state));
    return workflows;
  }, [workflows, filter]);

  // Group by kind
  const groups = useMemo<Array<{ heading: string; items: WorkflowTask[] }>>(() => {
    const pipelines = filtered.filter(
      (t) => t.workflowKind === "explore_plan_build" || t.workflowKind === "workflow",
    );
    const singleStage = filtered.filter(
      (t) => t.workflowKind === "explore" || t.workflowKind === "plan",
    );
    const result = [];
    if (pipelines.length > 0)
      result.push({ heading: "Pipeline workflows", items: pipelines });
    if (singleStage.length > 0)
      result.push({ heading: "Stage tasks", items: singleStage });
    return result;
  }, [filtered]);

  if (!companyId) {
    return (
      <div className="flex h-64 items-center justify-center text-sm text-[#555555]">
        No company selected.
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* Page header */}
      <div className="shrink-0 border-b border-[#1a1a1a] bg-[#060606] px-6 py-4">
        <div className="flex items-center gap-3">
          <GitBranch className="size-5 text-[#555555]" strokeWidth={1.5} />
          <div>
            <h1 className="font-mono text-[13px] font-medium uppercase tracking-widest text-[#c8c8c8]">
              Workflows
            </h1>
            <p className="text-[11px] text-[#555555]">
              Pipeline workflows and staged tasks (Explore → Plan → Build)
            </p>
          </div>
          <div className="ml-auto flex items-center gap-1">
            {FILTER_OPTIONS.map((opt) => (
              <button
                key={opt.value}
                type="button"
                onClick={() => setFilter(opt.value)}
                className={cn(
                  "rounded px-3 py-1 font-mono text-[10px] uppercase tracking-wide transition-colors",
                  filter === opt.value
                    ? "bg-[#1a1a1a] text-[#c8c8c8]"
                    : "text-[#555555] hover:text-[#888888]",
                )}
              >
                {opt.label}
              </button>
            ))}
          </div>
        </div>
      </div>

      {/* Content */}
      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-6">
        {isLoading ? (
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {Array.from({ length: 3 }).map((_, i) => (
              <Skeleton key={i} className="h-40 rounded-xl border border-[#1a1a1a] bg-[#080808]" />
            ))}
          </div>
        ) : groups.length === 0 ? (
          <div className="flex flex-col items-center justify-center gap-4 py-20 text-center">
            <GitBranch className="size-8 text-[#2a2a2a]" strokeWidth={1} />
            <div>
              <p className="text-sm text-[#555555]">No workflow tasks yet.</p>
              <p className="mt-1 text-[11px] text-[#3a3a3a]">
                Create a task with type <span className="font-mono text-[10px] text-[#555555]">Explore</span> or{" "}
                <span className="font-mono text-[10px] text-[#555555]">Workflow</span> from the Issues page.
              </p>
            </div>
            <Badge
              variant="outline"
              className="border-[#1a1a1a] bg-[#0a0a0a] font-mono text-[10px] text-[#444444]"
            >
              Issues → New issue → Explore / Workflow
            </Badge>
          </div>
        ) : (
          <div className="space-y-8">
            {groups.map((group) => (
              <div key={group.heading} className="space-y-3">
                <h2 className="font-mono text-[10px] uppercase tracking-widest text-[#444444]">
                  {group.heading}
                  <span className="ml-2 text-[#2a2a2a]">({group.items.length})</span>
                </h2>
                <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
                  {group.items.map((task) => (
                    <WorkflowCard
                      key={task.id}
                      task={task}
                      apiBase={apiBase}
                      companyId={companyId}
                      onRefresh={() => void refetch()}
                    />
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
