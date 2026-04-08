"use client";

import { Suspense, useEffect, useMemo } from "react";
import { useSearchParams } from "next/navigation";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Badge } from "@/app/components/ui/badge";
import { Button } from "@/app/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/app/components/ui/card";
import { WorkspaceNewIssueForm } from "@/app/components/console/WorkspaceNewIssueForm";
import { ScrollArea } from "@/app/components/ui/scroll-area";
import { Skeleton } from "@/app/components/ui/skeleton";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { capabilityRefsFromTask, workspaceAttachmentPathsFromTask } from "@/app/components/TaskListPanel";
import { taskToPcIssue } from "@/app/lib/hsm-api-adapter";
import type { HsmTaskRow } from "@/app/lib/hsm-api-types";
import { useCompanyAgents, useCompanyTasks } from "@/app/lib/hsm-queries";
import {
  buildIssueSpecFromPlan,
  buildIssueTitleFromPlan,
  isDoneTask,
  isPlanTask,
  truncatePath,
} from "@/app/lib/workspace-issue";

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
  if (f === "in_progress") {
    if (!(/progress|doing|active/i.test(task.state) || !!task.checked_out_by)) return false;
  }
  if (f === "open") {
    if (!/open|todo|pending/i.test(task.state)) return false;
  }
  if (f === "blocked") {
    if (!/block/i.test(task.state)) return false;
  }
  if (f === "completed") {
    if (!/done|complete|closed/i.test(task.state)) return false;
  }

  const view = sp.get("view");
  if (view === "waiting_admin" || view === "pending_approvals") {
    if (task.decision_mode !== "admin_required") return false;
  }
  if (view === "human_inbox") {
    const needs =
      task.requires_human === true ||
      task.state === "waiting_admin" ||
      task.state === "blocked";
    if (!needs) return false;
  }

  const project = sp.get("project");
  if (project && project !== "") {
    if (task.project_id !== project) return false;
  }

  return true;
}

function IssuesContent() {
  const searchParams = useSearchParams();
  const qc = useQueryClient();
  const { apiBase, companyId, companies, setPropertiesSelection } = useWorkspace();
  const company = companies.find((c) => c.id === companyId);
  const prefix = (company?.issue_key_prefix ?? "HSM").toUpperCase();

  const { data: tasks = [], isLoading, error } = useCompanyTasks(apiBase, companyId);
  const { data: agentsRaw = [] } = useCompanyAgents(apiBase, companyId);

  const firstAgent = useMemo(() => {
    const active = agentsRaw.filter((a) => (a.status ?? "").toLowerCase() !== "terminated");
    return active.sort((a, b) => a.name.localeCompare(b.name))[0];
  }, [agentsRaw]);

  const assigneePersona = firstAgent?.name ?? "";
  const assigneeDisplayName =
    firstAgent?.title ?? firstAgent?.name ?? company?.display_name ?? "Agent";

  /** Build child task from an approved plan row (inline “Build”). */
  const createTask = useMutation({
    mutationFn: async (overrides: { title: string; specification?: string | null; parentTaskId?: string }) => {
      const title = overrides.title.trim();
      if (!title) throw new Error("Title is required.");
      const body: Record<string, unknown> = {
        title,
        specification: overrides.specification ?? null,
      };
      if (overrides.parentTaskId) body.parent_task_id = overrides.parentTaskId;
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/tasks`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      const j = (await r.json().catch(() => ({}))) as { error?: string; task?: { id?: string } };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["hsm", "tasks", apiBase, companyId] });
      void qc.invalidateQueries({ queryKey: ["hsm", "intelligence", apiBase, companyId] });
    },
  });

  const filtered = useMemo(() => {
    const sp = new URLSearchParams(searchParams.toString());
    const hasFilters =
      sp.has("ids") ||
      sp.has("priority") ||
      sp.has("state") ||
      sp.has("filter") ||
      sp.has("view") ||
      sp.has("project");
    if (!hasFilters) return tasks;
    return tasks.filter((t) => taskMatchesQuery(t, sp));
  }, [tasks, searchParams]);

  const issues = useMemo(() => filtered.map((t) => taskToPcIssue(t, prefix)), [filtered, prefix]);

  useEffect(() => {
    const id = searchParams.get("focus");
    if (!id || tasks.length === 0) return;
    const t = tasks.find((x) => x.id === id);
    if (!t) return;
    const pc = taskToPcIssue(t, prefix);
    setPropertiesSelection({ kind: "task", id: t.id, title: `${pc.identifier} · ${t.title}` });
    requestAnimationFrame(() => {
      document.querySelector(`[data-task-row="${id}"]`)?.scrollIntoView({ block: "nearest", behavior: "smooth" });
    });
  }, [searchParams, tasks, prefix, setPropertiesSelection]);

  if (!companyId) {
    return <p className="pc-page-desc">Select a company in the header.</p>;
  }

  if (error) {
    return (
      <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-4 text-sm">
        {error instanceof Error ? error.message : String(error)}
      </div>
    );
  }

  const filterHint = searchParams.toString() ? (
    <span className="mt-1 block font-mono text-[10px] text-muted-foreground">
      Filtered: {searchParams.toString()}
    </span>
  ) : null;

  return (
    <div className="space-y-4">
      <div>
        <p className="pc-page-eyebrow">Company OS</p>
        <h1 className="pc-page-title">Issues</h1>
        <p className="pc-page-desc">
          Open work for this company: each row is a task with a short id like{" "}
          <span className="font-mono text-xs">
            {prefix}-42
          </span>{" "}
          (your company prefix plus a number). Tags under a title are skills, labels, plan vs executable work, reviewers,
          and similar — they tell agents and automation how to treat the task.
        </p>
        {filterHint}
      </div>

      <Card className="pc-panel border-admin-border">
        <CardHeader className="pb-2">
          <CardTitle className="text-base">New issue</CardTitle>
          <CardDescription>
            Create a task: plan vs task, project, priority, labels, who reviews, optional repeat cadence, and file paths
            under the company workspace. Attached paths are added to the description and linked on the task.
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
          <CardTitle className="text-base">Task list</CardTitle>
          <CardDescription>{isLoading ? "Loading…" : `${issues.length} issues`}</CardDescription>
        </CardHeader>
        <CardContent className="p-0">
          <ScrollArea className="h-[min(70vh,560px)]">
            <div className="min-w-[800px]">
              <div className="pc-table-header grid-cols-[100px_1fr_44px_120px_100px_80px_120px]">
                <span>Key</span>
                <span>Title</span>
                <span>Pri</span>
                <span>Status</span>
                <span>Gate</span>
                <span>Mode</span>
                <span>Assignee</span>
              </div>
              {isLoading
                ? Array.from({ length: 8 }).map((_, i) => (
                    <div key={i} className="flex gap-2 border-b border-admin-border px-4 py-3">
                      <Skeleton className="h-4 w-20" />
                      <Skeleton className="h-4 flex-1" />
                    </div>
                  ))
                : issues.map((issue) => {
                    const task = filtered.find((x) => x.id === issue.id);
                    const caps = task ? capabilityRefsFromTask(task) : [];
                    const wsPaths = task ? workspaceAttachmentPathsFromTask(task) : [];
                    return (
                      <div key={issue.id} className="border-b border-admin-border last:border-b-0">
                        {wsPaths.length > 0 ? (
                          <div
                            className="border-b border-amber-500/25 bg-amber-500/10 px-4 py-1.5 text-left text-[11px] text-amber-950 dark:border-amber-400/20 dark:bg-amber-400/10 dark:text-amber-100"
                            role="status"
                          >
                            <span className="font-medium">Workspace file attached</span>
                            <span className="mt-0.5 block font-mono text-[10px] opacity-90">
                              {wsPaths.length === 1
                                ? truncatePath(wsPaths[0]!)
                                : `${wsPaths.length} paths · ${truncatePath(wsPaths[0]!)}`}
                            </span>
                          </div>
                        ) : null}
                        <div
                          data-task-row={issue.id}
                          className="pc-table-row grid w-full grid-cols-[100px_1fr_44px_120px_100px_80px_120px] gap-2 border-b-0"
                        >
                          <button
                            type="button"
                            className="font-mono text-xs text-primary text-left"
                            onClick={() =>
                              setPropertiesSelection({
                                kind: "task",
                                id: issue.id,
                                title: `${issue.identifier} · ${issue.title}`,
                              })
                            }
                          >
                            {issue.identifier}
                          </button>
                          <button
                            type="button"
                            className="min-w-0 text-left"
                            onClick={() =>
                              setPropertiesSelection({
                                kind: "task",
                                id: issue.id,
                                title: `${issue.identifier} · ${issue.title}`,
                              })
                            }
                          >
                            <span className="block truncate font-medium text-foreground">{issue.title}</span>
                            {caps.filter((c) => c.kind !== "mode").length ? (
                              <span className="mt-1 flex flex-wrap gap-1">
                                {caps.filter((c) => c.kind !== "mode").slice(0, 4).map((c) => (
                                  <Badge
                                    key={`${issue.id}-${c.kind}-${c.ref}`}
                                    variant="outline"
                                    className="border-emerald-500/40 font-mono text-[9px] text-emerald-600 dark:text-emerald-400"
                                  >
                                    {c.kind}:{c.ref.length > 20 ? `${c.ref.slice(0, 18)}…` : c.ref}
                                  </Badge>
                                ))}
                                {caps.filter((c) => c.kind !== "mode").length > 4 ? (
                                  <span className="self-center font-mono text-[9px] text-muted-foreground">
                                    +{caps.filter((c) => c.kind !== "mode").length - 4}
                                  </span>
                                ) : null}
                              </span>
                            ) : null}
                          </button>
                          <span className="font-mono text-xs text-muted-foreground">{issue.priority}</span>
                          <span>
                            <Badge variant="secondary" className="font-mono text-[10px]">
                              {issue.status}
                            </Badge>
                          </span>
                          <span>
                            {issue.decisionMode === "admin_required" ? (
                              <Badge variant="outline" className="border-warn/50 font-mono text-[9px] text-warn">
                                admin
                              </Badge>
                            ) : (
                              <span className="text-xs text-muted-foreground">—</span>
                            )}
                          </span>
                          <span>
                            {task && isPlanTask(task) ? (
                              isDoneTask(task) ? (
                                <Button
                                  variant="outline"
                                  size="xs"
                                  className="border-violet-500/50 font-mono text-[9px] text-violet-400 hover:bg-violet-500/10"
                                  disabled={createTask.isPending}
                                  onClick={() => {
                                    createTask.mutate({
                                      title: buildIssueTitleFromPlan(task.title),
                                      specification: buildIssueSpecFromPlan(task.specification),
                                      parentTaskId: task.id,
                                    });
                                  }}
                                >
                                  Build
                                </Button>
                              ) : (
                                <Badge variant="outline" className="border-violet-500/40 font-mono text-[9px] text-violet-400">
                                  plan
                                </Badge>
                              )
                            ) : (
                              <span className="text-xs text-muted-foreground">—</span>
                            )}
                          </span>
                          <span className="truncate font-mono text-xs text-muted-foreground">
                            {issue.assigneeId ?? "—"}
                          </span>
                        </div>
                      </div>
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
