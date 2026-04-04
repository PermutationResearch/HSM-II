"use client";

import Link from "next/link";
import { Badge } from "@/app/components/ui/badge";
import { Button } from "@/app/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/app/components/ui/card";
import { ScrollArea } from "@/app/components/ui/scroll-area";
import { Skeleton } from "@/app/components/ui/skeleton";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { taskToPcIssue } from "@/app/lib/hsm-api-adapter";
import type { HsmTaskRow } from "@/app/lib/hsm-api-types";
import { useCompanyTasks } from "@/app/lib/hsm-queries";

function needsApproval(task: HsmTaskRow): boolean {
  return task.decision_mode === "admin_required";
}

export default function WorkspaceApprovalsPage() {
  const { apiBase, companyId, companies, setPropertiesSelection } = useWorkspace();
  const company = companies.find((c) => c.id === companyId);
  const prefix = (company?.issue_key_prefix ?? "HSM").toUpperCase();

  const { data: tasks = [], isLoading, error } = useCompanyTasks(apiBase, companyId);

  const pending = tasks
    .filter(needsApproval)
    .sort((a, b) => (b.priority ?? 0) - (a.priority ?? 0));
  const pcRows = pending.map((t) => ({ task: t, issue: taskToPcIssue(t, prefix) }));

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

  return (
    <div className="space-y-4">
      <div>
        <p className="pc-page-eyebrow">Control plane</p>
        <h1 className="pc-page-title">Approvals</h1>
        <p className="pc-page-desc">
          Tasks with <span className="font-mono text-xs">decision_mode = admin_required</span> after policy evaluation.
          Resolve in the{" "}
          <Link href="/workspace/issues?view=pending_approvals" className="text-primary underline-offset-4 hover:underline">
            Issues
          </Link>{" "}
          queue or legacy console for task decisions.
        </p>
      </div>

      <Card className="pc-panel border-admin-border">
        <CardHeader className="pb-2">
          <CardTitle className="text-base">Pending board / admin attention</CardTitle>
          <CardDescription>
            {isLoading ? "Loading…" : `${pending.length} task${pending.length === 1 ? "" : "s"}`}
          </CardDescription>
        </CardHeader>
        <CardContent className="p-0">
          <ScrollArea className="h-[min(70vh,560px)]">
            <div className="min-w-[720px]">
              <div className="pc-table-header grid-cols-[120px_1fr_56px_120px_120px]">
                <span>Key</span>
                <span>Title</span>
                <span>Pri</span>
                <span>Status</span>
                <span>Actions</span>
              </div>
              {isLoading
                ? Array.from({ length: 6 }).map((_, i) => (
                    <div key={i} className="flex gap-2 border-b border-admin-border px-4 py-3">
                      <Skeleton className="h-4 w-20" />
                      <Skeleton className="h-4 flex-1" />
                    </div>
                  ))
                : pcRows.length === 0
                  ? (
                      <p className="px-4 py-8 text-sm text-muted-foreground">
                        No tasks awaiting admin approval for this company.
                      </p>
                    )
                  : pcRows.map(({ task, issue }) => (
                      <div
                        key={task.id}
                        className="pc-table-row grid grid-cols-[120px_1fr_56px_120px_120px] gap-2 items-center"
                      >
                        <span className="font-mono text-xs text-primary">{issue.identifier}</span>
                        <button
                          type="button"
                          className="truncate text-left font-medium text-foreground hover:underline"
                          onClick={() =>
                            setPropertiesSelection({
                              kind: "task",
                              id: task.id,
                              title: `${issue.identifier} · ${issue.title}`,
                            })
                          }
                        >
                          {issue.title}
                        </button>
                        <span className="font-mono text-xs text-muted-foreground">{issue.priority}</span>
                        <span>
                          <Badge variant="secondary" className="font-mono text-[10px]">
                            {issue.status}
                          </Badge>
                        </span>
                        <span>
                          <Button variant="outline" size="xs" asChild>
                            <Link href={`/workspace/issues?focus=${encodeURIComponent(task.id)}`}>Open in Issues</Link>
                          </Button>
                        </span>
                      </div>
                    ))}
            </div>
          </ScrollArea>
        </CardContent>
      </Card>
    </div>
  );
}
