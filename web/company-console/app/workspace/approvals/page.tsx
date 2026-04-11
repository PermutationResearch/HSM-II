"use client";

import Link from "next/link";
import { useQuery } from "@tanstack/react-query";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Badge } from "@/app/components/ui/badge";
import { Button } from "@/app/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/app/components/ui/card";
import { ScrollArea } from "@/app/components/ui/scroll-area";
import { Skeleton } from "@/app/components/ui/skeleton";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { companyOsUrl } from "@/app/lib/company-api-url";
import { useOperatorInbox } from "@/app/lib/hsm-queries";

export default function WorkspaceApprovalsPage() {
  const qc = useQueryClient();
  const { apiBase, companyId, setPropertiesSelection } = useWorkspace();
  const operatorInbox = useOperatorInbox(apiBase, companyId);
  const isLoading = operatorInbox.isLoading;
  const error = operatorInbox.error;
  const items = operatorInbox.data?.items ?? [];
  const taskItems = items.filter((item) => item.kind === "task");
  const emailItems = items.filter((item) => item.kind === "email");
  const failureItems = items.filter((item) => item.kind === "failure");
  const runInbox = useQuery({
    queryKey: ["hsm", "agent-runs-inbox", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(
        companyOsUrl(apiBase, `/api/company/companies/${companyId}/agent-runs?operator_inbox=true&limit=30`),
      );
      const j = (await r.json().catch(() => ({}))) as {
        runs?: Array<{
          id: string;
          task_id?: string | null;
          status: string;
          summary?: string | null;
          external_system?: string;
          started_at?: string;
          meta?: Record<string, unknown>;
        }>;
        error?: string;
      };
      if (!r.ok) throw new Error(j.error ?? `runs ${r.status}`);
      return j.runs ?? [];
    },
    enabled: !!companyId,
    refetchInterval: 10_000,
  });

  const decideTask = useMutation({
    mutationFn: async ({ taskId, decision_mode }: { taskId: string; decision_mode: "auto" | "blocked" }) => {
      const r = await fetch(companyOsUrl(apiBase, `/api/company/tasks/${taskId}/decision`), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          decision_mode,
          actor: "approvals_ui",
          reason: decision_mode === "auto" ? "approved_in_ui" : "rejected_in_ui",
        }),
      });
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `decision ${r.status}`);
      return j;
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["hsm", "operator-inbox", apiBase, companyId] });
      void qc.invalidateQueries({ queryKey: ["hsm", "tasks", apiBase, companyId] });
      void qc.invalidateQueries({ queryKey: ["hsm", "ops-overview", apiBase, companyId] });
    },
  });

  const decideEmail = useMutation({
    mutationFn: async ({ itemId, decision }: { itemId: string; decision: "approve" | "reject" }) => {
      const r = await fetch(companyOsUrl(apiBase, `/api/company/email/operator-queue/${itemId}/decision`), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          decision,
          actor: "company_owner",
          reason: decision === "approve" ? "approved_in_ui" : "rejected_in_ui",
        }),
      });
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `email decision ${r.status}`);
      return j;
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["hsm", "operator-inbox", apiBase, companyId] });
      void qc.invalidateQueries({ queryKey: ["hsm", "ops-overview", apiBase, companyId] });
    },
  });

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
        <h1 className="pc-page-title">Operator Inbox</h1>
        <p className="pc-page-desc">
          Unified queue for approvals, email drafts, and failures with adaptive lanes by company profile.
          Continue detailed triage in{" "}
          <Link href="/workspace/issues?view=pending_approvals" className="text-primary underline-offset-4 hover:underline">
            Issues
          </Link>.
        </p>
      </div>

      <Card className="pc-panel border-admin-border">
        <CardHeader className="pb-2">
          <CardTitle className="text-base">Adaptive lanes</CardTitle>
          <CardDescription>
            {(operatorInbox.data?.profile?.size_tier ?? "solo")} mode · {operatorInbox.data?.counts?.total ?? 0} total items
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-2 md:grid-cols-3">
          {(operatorInbox.data?.lanes ?? []).map((lane) => (
            <div key={lane.id} className="rounded-xl border border-admin-border bg-black/10 p-3">
              <p className="text-sm font-medium text-foreground">{lane.label}</p>
              <p className="mt-1 text-[11px] text-muted-foreground">
                kinds: {lane.item_kinds.join(", ")} {lane.sla ? `· SLA ${lane.sla}` : ""}
              </p>
            </div>
          ))}
        </CardContent>
      </Card>

      <Card className="pc-panel border-admin-border">
        <CardHeader className="pb-2">
          <CardTitle className="text-base">Runs needing operator attention</CardTitle>
          <CardDescription>
            {(runInbox.data?.length ?? 0) > 0
              ? `${runInbox.data?.length ?? 0} run(s) flagged by status or requires_human`
              : "No run-level escalations right now"}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-2">
          {runInbox.isLoading ? (
            <Skeleton className="h-20 w-full" />
          ) : runInbox.error ? (
            <p className="text-sm text-destructive">
              {runInbox.error instanceof Error ? runInbox.error.message : String(runInbox.error)}
            </p>
          ) : (runInbox.data?.length ?? 0) === 0 ? (
            <p className="text-sm text-muted-foreground">No escalated runs in the current window.</p>
          ) : (
            runInbox.data?.map((run) => {
              const mode =
                typeof run.meta?.execution_mode === "string" ? run.meta.execution_mode : "unknown";
              return (
                <div key={run.id} className="rounded-lg border border-admin-border bg-black/10 px-3 py-2">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <p className="text-sm font-medium text-foreground">
                      {run.summary?.trim() || `Run ${run.id.slice(0, 8)}`}
                    </p>
                    <Badge variant={run.status === "error" ? "destructive" : "outline"} className="font-mono text-[10px]">
                      {run.status}
                    </Badge>
                  </div>
                  <p className="mt-1 font-mono text-[10px] text-muted-foreground">
                    {run.external_system ?? "system"} · mode {mode}
                    {run.task_id ? ` · task ${run.task_id.slice(0, 8)}…` : ""}
                  </p>
                  <div className="mt-2">
                    {run.task_id ? (
                      <Button
                        variant="outline"
                        size="xs"
                        onClick={() =>
                          setPropertiesSelection({
                            kind: "task",
                            id: String(run.task_id),
                            title: run.summary?.trim() || `Run ${run.id.slice(0, 8)}`,
                          })
                        }
                      >
                        Open task
                      </Button>
                    ) : null}
                  </div>
                </div>
              );
            })
          )}
        </CardContent>
      </Card>

      <Card className="pc-panel border-admin-border">
        <CardHeader className="pb-2">
          <CardTitle className="text-base">Email drafts awaiting owner confirmation</CardTitle>
          <CardDescription>
            {isLoading
              ? "Loading…"
              : `${emailItems.length} email draft${emailItems.length === 1 ? "" : "s"}`}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-2">
          {emailItems.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              No pending email replies right now. Connect business email and let agents propose drafts here.
            </p>
          ) : (
            emailItems.map((item) => (
              <div key={item.id} className="rounded-xl border border-admin-border bg-black/10 p-3">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <p className="text-sm font-medium text-foreground">{item.title}</p>
                  <span className="font-mono text-[10px] text-muted-foreground">{item.mailbox ?? "mailbox"}</span>
                </div>
                <p className="mt-1 text-xs text-muted-foreground">From: {item.from_address ?? "unknown"}</p>
                <p className="mt-2 line-clamp-3 text-xs text-muted-foreground">{item.body_text ?? "No body preview."}</p>
                <div className="mt-2 rounded-md border border-admin-border/70 bg-black/20 p-2">
                  <p className="text-[10px] uppercase tracking-wide text-muted-foreground">Suggested reply</p>
                  <p className="mt-1 whitespace-pre-wrap text-xs text-foreground">
                    {item.suggested_reply ?? "No draft yet. Agent needs to propose a reply first."}
                  </p>
                </div>
                <div className="mt-3 flex flex-wrap gap-2">
                  <Button
                    size="xs"
                    variant="secondary"
                    disabled={!item.suggested_reply || decideEmail.isPending}
                    onClick={() => decideEmail.mutate({ itemId: item.id, decision: "approve" })}
                  >
                    Approve + Send
                  </Button>
                  <Button
                    size="xs"
                    variant="outline"
                    disabled={decideEmail.isPending}
                    onClick={() => decideEmail.mutate({ itemId: item.id, decision: "reject" })}
                  >
                    Reject
                  </Button>
                </div>
              </div>
            ))
          )}
        </CardContent>
      </Card>

      <Card className="pc-panel border-admin-border">
        <CardHeader className="pb-2">
          <CardTitle className="text-base">Pending board / admin attention</CardTitle>
          <CardDescription>
            {isLoading ? "Loading…" : `${taskItems.length} task${taskItems.length === 1 ? "" : "s"}`}
          </CardDescription>
        </CardHeader>
        <CardContent className="p-0">
          <ScrollArea className="h-[min(70vh,560px)]">
            <div className="min-w-[720px]">
              <div className="pc-table-header grid-cols-[120px_1fr_56px_120px_220px]">
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
                : taskItems.length === 0
                  ? (
                      <p className="px-4 py-8 text-sm text-muted-foreground">
                        No tasks awaiting admin approval for this company.
                      </p>
                    )
                  : taskItems.map((task) => (
                      <div
                        key={task.id}
                        className="pc-table-row grid grid-cols-[120px_1fr_56px_120px_220px] gap-2 items-center"
                      >
                        <span className="font-mono text-xs text-primary">{String(task.id).slice(0, 8)}</span>
                        <button
                          type="button"
                          className="truncate text-left font-medium text-foreground hover:underline"
                          onClick={() =>
                            setPropertiesSelection({
                              kind: "task",
                              id: String(task.id),
                              title: String(task.title ?? "Task"),
                            })
                          }
                        >
                          {String(task.title)}
                        </button>
                        <span className="font-mono text-xs text-muted-foreground">{String(task.priority ?? "0")}</span>
                        <span>
                          <Badge variant="secondary" className="font-mono text-[10px]">
                            {String(task.state)}
                          </Badge>
                        </span>
                        <span>
                          <div className="flex flex-wrap gap-1.5">
                            <Button
                              variant="secondary"
                              size="xs"
                              disabled={decideTask.isPending}
                              onClick={() => decideTask.mutate({ taskId: String(task.id), decision_mode: "auto" })}
                            >
                              Approve
                            </Button>
                            <Button
                              variant="destructive"
                              size="xs"
                              disabled={decideTask.isPending}
                              onClick={() => decideTask.mutate({ taskId: String(task.id), decision_mode: "blocked" })}
                            >
                              Reject
                            </Button>
                            <Button variant="outline" size="xs" asChild>
                              <Link href={`/workspace/issues?focus=${encodeURIComponent(String(task.id))}`}>Open</Link>
                            </Button>
                          </div>
                        </span>
                      </div>
                    ))}
            </div>
          </ScrollArea>
        </CardContent>
      </Card>

      <Card className="pc-panel border-admin-border">
        <CardHeader className="pb-2">
          <CardTitle className="text-base">Reliability failures</CardTitle>
          <CardDescription>{failureItems.length} signals needing triage</CardDescription>
        </CardHeader>
        <CardContent className="space-y-2">
          {failureItems.length === 0 ? (
            <p className="text-sm text-muted-foreground">No recent failure telemetry in the inbox window.</p>
          ) : (
            failureItems.slice(0, 20).map((failure) => (
              <div key={failure.id} className="rounded-lg border border-admin-border bg-black/10 px-3 py-2">
                <p className="text-sm font-medium text-foreground">{String(failure.title)}</p>
                <p className="text-[11px] text-muted-foreground">confidence {String(failure.confidence ?? "n/a")}</p>
              </div>
            ))
          )}
        </CardContent>
      </Card>
    </div>
  );
}
