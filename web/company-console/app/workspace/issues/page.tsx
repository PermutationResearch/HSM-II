"use client";

import { Suspense, useEffect, useMemo, useState } from "react";
import { useSearchParams } from "next/navigation";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Badge } from "@/app/components/ui/badge";
import { Button } from "@/app/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/app/components/ui/card";
import { Input } from "@/app/components/ui/input";
import { ScrollArea } from "@/app/components/ui/scroll-area";
import { Skeleton } from "@/app/components/ui/skeleton";
import { Textarea } from "@/app/components/ui/textarea";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { capabilityRefsFromTask, workspaceAttachmentPathsFromTask } from "@/app/components/TaskListPanel";
import { taskToPcIssue } from "@/app/lib/hsm-api-adapter";
import type { HsmTaskRow } from "@/app/lib/hsm-api-types";
import { useCompanyTasks } from "@/app/lib/hsm-queries";
import {
  buildIssueSpecFromPlan,
  buildIssueTitleFromPlan,
  isDoneTask,
  isPlanTask,
  specificationWithWorkspacePaths,
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

  return true;
}

function IssuesContent() {
  const searchParams = useSearchParams();
  const qc = useQueryClient();
  const { apiBase, companyId, companies, setPropertiesSelection } = useWorkspace();
  const company = companies.find((c) => c.id === companyId);
  const prefix = (company?.issue_key_prefix ?? "HSM").toUpperCase();

  const { data: tasks = [], isLoading, error } = useCompanyTasks(apiBase, companyId);

  const [newTitle, setNewTitle] = useState("");
  const [newSpec, setNewSpec] = useState("");
  const [draftWorkspacePaths, setDraftWorkspacePaths] = useState<string[]>([]);
  const [pathDraft, setPathDraft] = useState("");
  const [newOwnerPersona, setNewOwnerPersona] = useState("");
  const [newIsPlan, setNewIsPlan] = useState(false);

  const createTask = useMutation({
    mutationFn: async (overrides?: { title?: string; specification?: string; parentTaskId?: string }) => {
      const title = (overrides?.title ?? newTitle).trim();
      if (!title) throw new Error("Title is required.");
      const paths = overrides ? [] : draftWorkspacePaths.map((p) => p.trim()).filter(Boolean);
      const specification = overrides?.specification ?? specificationWithWorkspacePaths(newSpec, paths);
      const body: Record<string, unknown> = {
        title,
        specification: specification || null,
        workspace_attachment_paths: paths.length ? paths : undefined,
      };
      const op = overrides ? undefined : newOwnerPersona.trim();
      if (op) body.owner_persona = op;
      if (overrides?.parentTaskId) body.parent_task_id = overrides.parentTaskId;
      if (!overrides && newIsPlan) {
        body.capability_refs = [{ kind: "mode", ref: "plan" }];
      }
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/tasks`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      const j = (await r.json().catch(() => ({}))) as { error?: string; task?: { id?: string } };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    onSuccess: (_data, overrides) => {
      if (!overrides) {
        setNewTitle("");
        setNewSpec("");
        setDraftWorkspacePaths([]);
        setPathDraft("");
        setNewOwnerPersona("");
        setNewIsPlan(false);
      }
      void qc.invalidateQueries({ queryKey: ["hsm", "tasks", apiBase, companyId] });
      void qc.invalidateQueries({ queryKey: ["hsm", "intelligence", apiBase, companyId] });
    },
  });

  function attachWorkspacePath() {
    const p = pathDraft.trim();
    if (!p) return;
    setDraftWorkspacePaths((prev) => (prev.includes(p) ? prev : [...prev, p]));
    setPathDraft("");
  }

  function removeDraftPath(path: string) {
    setDraftWorkspacePaths((prev) => prev.filter((x) => x !== path));
  }

  const filtered = useMemo(() => {
    const sp = new URLSearchParams(searchParams.toString());
    const hasFilters =
      sp.has("ids") ||
      sp.has("priority") ||
      sp.has("state") ||
      sp.has("filter") ||
      sp.has("view");
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
          Company OS tasks from <span className="font-mono text-xs">GET …/tasks</span> ({prefix}-N keys). Capability
          chips are <span className="font-mono text-xs">capability_refs</span> on each row.
        </p>
        {filterHint}
      </div>

      <Card className="pc-panel border-admin-border">
        <CardHeader className="pb-2">
          <CardTitle className="text-base">New issue</CardTitle>
          <CardDescription>
            Create a task via <span className="font-mono text-xs">POST …/tasks</span>. Attach workspace paths
            (relative to company <span className="font-mono text-xs">hsmii_home</span>) — each becomes a{" "}
            <span className="font-mono text-xs">Workspace file:</span> line in the spec and an entry in{" "}
            <span className="font-mono text-xs">workspace_attachment_paths</span>.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {draftWorkspacePaths.length > 0 ? (
            <div
              className="rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-950 dark:border-amber-400/35 dark:bg-amber-400/10 dark:text-amber-100"
              role="status"
            >
              <span className="font-medium">Workspace file attached</span>
              <span className="mt-0.5 block font-mono text-[11px] opacity-90">
                {draftWorkspacePaths.length === 1
                  ? truncatePath(draftWorkspacePaths[0]!)
                  : `${draftWorkspacePaths.length} paths · ${truncatePath(draftWorkspacePaths[0]!)}`}
              </span>
            </div>
          ) : null}
          <div className="grid gap-2 sm:grid-cols-[1fr_auto] sm:items-end">
            <div className="space-y-1">
              <label className="text-xs text-muted-foreground" htmlFor="issue-ws-path">
                Attach from workspace
              </label>
              <Input
                id="issue-ws-path"
                className="font-mono text-xs"
                placeholder="workspace/content/drafts/…"
                value={pathDraft}
                onChange={(e) => setPathDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    attachWorkspacePath();
                  }
                }}
              />
            </div>
            <Button type="button" variant="secondary" className="w-full sm:w-auto" onClick={attachWorkspacePath}>
              Attach path
            </Button>
          </div>
          {draftWorkspacePaths.length > 0 ? (
            <div className="flex flex-wrap gap-1">
              {draftWorkspacePaths.map((p) => (
                <span
                  key={p}
                  className="inline-flex max-w-full items-center gap-1 rounded-md border border-amber-500/40 bg-background/60 px-2 py-0.5 font-mono text-[10px]"
                  title={p}
                >
                  <span className="truncate">{truncatePath(p, 40)}</span>
                  <button
                    type="button"
                    className="shrink-0 text-muted-foreground hover:text-foreground"
                    aria-label={`Remove path ${p}`}
                    onClick={() => removeDraftPath(p)}
                  >
                    ×
                  </button>
                </span>
              ))}
            </div>
          ) : null}
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground" htmlFor="issue-title">
              Title
            </label>
            <Input
              id="issue-title"
              value={newTitle}
              onChange={(e) => setNewTitle(e.target.value)}
              placeholder="Short summary"
            />
          </div>
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground" htmlFor="issue-spec">
              Specification
            </label>
            <Textarea
              id="issue-spec"
              className="min-h-[100px] font-mono text-xs"
              value={newSpec}
              onChange={(e) => setNewSpec(e.target.value)}
              placeholder="Context, acceptance criteria, links…"
            />
          </div>
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground" htmlFor="issue-owner">
              Assignee / owner_persona (optional)
            </label>
            <Input
              id="issue-owner"
              className="font-mono text-xs"
              value={newOwnerPersona}
              onChange={(e) => setNewOwnerPersona(e.target.value)}
              placeholder="e.g. engineering_lead"
            />
          </div>
          <label className="flex items-center gap-2 text-xs text-muted-foreground">
            <input
              type="checkbox"
              checked={newIsPlan}
              onChange={(e) => setNewIsPlan(e.target.checked)}
              className="size-3.5 rounded border-muted-foreground"
            />
            Plan mode — once approved, click <strong>Build</strong> to create an implementation issue
          </label>
          {createTask.isError ? (
            <p className="text-sm text-destructive">
              {createTask.error instanceof Error ? createTask.error.message : String(createTask.error)}
            </p>
          ) : null}
          <Button
            type="button"
            disabled={!newTitle.trim() || createTask.isPending}
            onClick={() => createTask.mutate(undefined)}
          >
            {createTask.isPending ? "Creating…" : "Create issue"}
          </Button>
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
