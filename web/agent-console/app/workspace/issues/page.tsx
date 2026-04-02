"use client";

import { Suspense, useEffect, useMemo } from "react";
import { useSearchParams } from "next/navigation";
import { Badge } from "@/app/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/app/components/ui/card";
import { ScrollArea } from "@/app/components/ui/scroll-area";
import { Skeleton } from "@/app/components/ui/skeleton";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { taskToPcIssue } from "@/app/lib/hsm-api-adapter";
import type { HsmTaskRow } from "@/app/lib/hsm-api-types";
import { useCompanyTasks } from "@/app/lib/hsm-queries";

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

  return true;
}

function IssuesContent() {
  const searchParams = useSearchParams();
  const { apiBase, companyId, companies, setPropertiesSelection } = useWorkspace();
  const company = companies.find((c) => c.id === companyId);
  const prefix = (company?.issue_key_prefix ?? "HSM").toUpperCase();

  const { data: tasks = [], isLoading, error } = useCompanyTasks(apiBase, companyId);

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
          Tasks from <span className="font-mono text-xs">GET /api/company/companies/…/tasks</span> with Paperclip-style
          keys ({prefix}-N).
        </p>
        {filterHint}
      </div>

      <Card className="pc-panel border-admin-border">
        <CardHeader className="pb-2">
          <CardTitle className="text-base">Task list</CardTitle>
          <CardDescription>{isLoading ? "Loading…" : `${issues.length} issues`}</CardDescription>
        </CardHeader>
        <CardContent className="p-0">
          <ScrollArea className="h-[min(70vh,560px)]">
            <div className="min-w-[800px]">
              <div className="pc-table-header grid-cols-[100px_1fr_44px_120px_100px_120px]">
                <span>Key</span>
                <span>Title</span>
                <span>Pri</span>
                <span>Status</span>
                <span>Gate</span>
                <span>Assignee</span>
              </div>
              {isLoading
                ? Array.from({ length: 8 }).map((_, i) => (
                    <div key={i} className="flex gap-2 border-b border-admin-border px-4 py-3">
                      <Skeleton className="h-4 w-20" />
                      <Skeleton className="h-4 flex-1" />
                    </div>
                  ))
                : issues.map((issue) => (
                    <button
                      key={issue.id}
                      type="button"
                      data-task-row={issue.id}
                      className="pc-table-row grid w-full grid-cols-[100px_1fr_44px_120px_100px_120px] gap-2"
                      onClick={() =>
                        setPropertiesSelection({
                          kind: "task",
                          id: issue.id,
                          title: `${issue.identifier} · ${issue.title}`,
                        })
                      }
                    >
                      <span className="font-mono text-xs text-primary">{issue.identifier}</span>
                      <span className="truncate font-medium text-foreground">{issue.title}</span>
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
                      <span className="truncate font-mono text-xs text-muted-foreground">{issue.assigneeId ?? "—"}</span>
                    </button>
                  ))}
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
