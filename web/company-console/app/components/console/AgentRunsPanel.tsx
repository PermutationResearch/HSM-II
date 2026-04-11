"use client";

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, ArrowUpRight } from "lucide-react";
import { Badge } from "@/app/components/ui/badge";
import { Button } from "@/app/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/app/components/ui/card";
import { ScrollArea } from "@/app/components/ui/scroll-area";
import { Skeleton } from "@/app/components/ui/skeleton";
import { Textarea } from "@/app/components/ui/textarea";
import { Input } from "@/app/components/ui/input";
import { useSelfImprovementSummary } from "@/app/lib/hsm-queries";

// ── API types ────────────────────────────────────────────────────────────────

type AgentRun = {
  id: string;
  company_id: string;
  task_id: string | null;
  company_agent_id: string | null;
  external_run_id: string | null;
  external_system: string;
  status: "running" | "success" | "error" | "cancelled";
  started_at: string;
  finished_at: string | null;
  summary: string | null;
  meta: Record<string, unknown>;
};

type FeedbackEvent = {
  id: string;
  run_id: string;
  step_index: number | null;
  actor: string;
  kind: "comment" | "correction" | "blocker" | "praise";
  body: string;
  created_at: string;
  spawned_task_id: string | null;
};

// ── Helpers ───────────────────────────────────────────────────────────────────

function statusVariant(
  status: AgentRun["status"]
): "default" | "secondary" | "destructive" | "outline" {
  if (status === "success") return "default";
  if (status === "running") return "secondary";
  if (status === "error") return "destructive";
  return "outline";
}

function kindVariant(
  kind: FeedbackEvent["kind"]
): "default" | "secondary" | "destructive" | "outline" {
  if (kind === "praise") return "default";
  if (kind === "comment") return "secondary";
  if (kind === "blocker") return "destructive";
  return "outline"; // correction
}

function fmtTs(ts: string | null): string {
  if (!ts) return "—";
  try {
    return new Date(ts).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return ts;
  }
}

function executionMode(run: AgentRun): "worker" | "llm_simulated" | "pending" | "unknown" {
  const raw = typeof run.meta?.execution_mode === "string" ? run.meta.execution_mode : "";
  if (raw === "worker" || raw === "llm_simulated" || raw === "pending") return raw;
  return "unknown";
}

// ── Run detail (feedback + promote) ──────────────────────────────────────────

function RunDetail({
  apiBase,
  companyId,
  run,
  onPromoted,
}: {
  apiBase: string;
  companyId: string;
  run: AgentRun;
  onPromoted: () => void;
}) {
  const qc = useQueryClient();
  const runKey = ["hsm", "agent-run", apiBase, companyId, run.id];

  const { data, isLoading } = useQuery({
    queryKey: runKey,
    queryFn: async () => {
      const r = await fetch(
        `${apiBase}/api/company/companies/${companyId}/agent-runs/${run.id}`
      );
      if (!r.ok) throw new Error(`${r.status}`);
      return r.json() as Promise<{ run: AgentRun; feedback: FeedbackEvent[] }>;
    },
    refetchInterval: run.status === "running" ? 8000 : false,
  });

  const [feedBody, setFeedBody] = useState("");
  const [feedKind, setFeedKind] = useState<FeedbackEvent["kind"]>("comment");
  const [feedStep, setFeedStep] = useState("");
  const [promoteEventId, setPromoteEventId] = useState<string | null>(null);
  const [promoteTitle, setPromoteTitle] = useState("");
  const [promoteOwner, setPromoteOwner] = useState("");

  const invalidate = () => void qc.invalidateQueries({ queryKey: runKey });

  const addFeedback = useMutation({
    mutationFn: async () => {
      const r = await fetch(
        `${apiBase}/api/company/companies/${companyId}/agent-runs/${run.id}/feedback`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            actor: "operator",
            body: feedBody.trim(),
            kind: feedKind,
            step_index: feedStep !== "" ? parseInt(feedStep, 10) : undefined,
          }),
        }
      );
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    onSuccess: () => {
      setFeedBody("");
      setFeedStep("");
      invalidate();
    },
  });

  const promoteTask = useMutation({
    mutationFn: async (eventId: string) => {
      const r = await fetch(
        `${apiBase}/api/company/companies/${companyId}/agent-runs/${run.id}/feedback/${eventId}/promote-task`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            title: promoteTitle.trim() || "Task from run feedback",
            owner_persona: promoteOwner.trim() || undefined,
          }),
        }
      );
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    onSuccess: () => {
      setPromoteEventId(null);
      setPromoteTitle("");
      setPromoteOwner("");
      invalidate();
      onPromoted();
    },
  });

  const feedback = data?.feedback ?? [];

  return (
    <div className="space-y-4 px-1 pb-1 pt-2">
      {/* Meta strip */}
      <div className="flex flex-wrap gap-x-4 gap-y-1 text-[10px] text-muted-foreground font-mono">
        <span>started {fmtTs(run.started_at)}</span>
        {run.finished_at && <span>finished {fmtTs(run.finished_at)}</span>}
        <span>mode {executionMode(run)}</span>
        {run.summary && <span className="text-foreground/80">{run.summary}</span>}
        {run.external_run_id && (
          <span>
            {run.external_system}:{run.external_run_id}
          </span>
        )}
      </div>

      {/* Feedback thread */}
      <div>
        <p className="mb-2 text-[11px] font-medium text-foreground/70">
          Feedback ({feedback.length})
        </p>
        {isLoading ? (
          <Skeleton className="h-16 w-full" />
        ) : feedback.length === 0 ? (
          <p className="text-[11px] text-muted-foreground">
            No feedback yet. Add the first observation below.
          </p>
        ) : (
          <div className="space-y-2">
            {feedback.map((ev) => (
              <div
                key={ev.id}
                className="rounded border border-admin-border bg-black/10 p-2"
              >
                <div className="flex flex-wrap items-center gap-1.5">
                  <Badge variant={kindVariant(ev.kind)} className="text-[9px]">
                    {ev.kind}
                  </Badge>
                  {ev.step_index != null && (
                    <span className="font-mono text-[9px] text-muted-foreground">
                      step {ev.step_index}
                    </span>
                  )}
                  <span className="font-mono text-[9px] text-muted-foreground">
                    {ev.actor} · {fmtTs(ev.created_at)}
                  </span>
                  {ev.spawned_task_id ? (
                    <Badge variant="outline" className="text-[9px]">
                      → task {ev.spawned_task_id.slice(0, 8)}…
                    </Badge>
                  ) : (
                    <button
                      type="button"
                      className="ml-auto flex items-center gap-0.5 rounded border border-admin-border px-1.5 py-0.5 font-mono text-[9px] text-muted-foreground hover:bg-white/5 hover:text-foreground"
                      onClick={() => {
                        setPromoteEventId(ev.id);
                        setPromoteTitle(
                          ev.kind === "blocker"
                            ? `Fix blocker: ${ev.body.slice(0, 60)}`
                            : ev.body.slice(0, 80)
                        );
                      }}
                    >
                      <ArrowUpRight className="h-2.5 w-2.5" />
                      Promote to task
                    </button>
                  )}
                </div>
                <p className="mt-1 text-[11px] text-foreground/90">{ev.body}</p>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Promote form */}
      {promoteEventId && (
        <div className="rounded border border-[#3b82f6]/30 bg-[#3b82f6]/5 p-3 space-y-2">
          <p className="text-[11px] font-medium text-[#79b8ff]">Promote to task</p>
          <Input
            className="h-8 border-admin-border bg-black/20 font-mono text-xs"
            placeholder="Task title…"
            value={promoteTitle}
            onChange={(e) => setPromoteTitle(e.target.value)}
          />
          <Input
            className="h-8 border-admin-border bg-black/20 font-mono text-xs"
            placeholder="Assign to (agent/persona, optional)…"
            value={promoteOwner}
            onChange={(e) => setPromoteOwner(e.target.value)}
          />
          <div className="flex gap-2">
            <Button
              size="sm"
              variant="outline"
              className="h-7 border-[#3b82f6]/40 text-[11px] text-[#79b8ff]"
              disabled={!promoteTitle.trim() || promoteTask.isPending}
              onClick={() => promoteTask.mutate(promoteEventId)}
            >
              {promoteTask.isPending ? "Creating…" : "Create task"}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              className="h-7 text-[11px]"
              onClick={() => setPromoteEventId(null)}
            >
              Cancel
            </Button>
          </div>
          {promoteTask.error && (
            <p className="text-[11px] text-destructive">
              {promoteTask.error instanceof Error
                ? promoteTask.error.message
                : "Failed"}
            </p>
          )}
        </div>
      )}

      {/* Add feedback */}
      <div className="space-y-2 rounded border border-admin-border p-3">
        <p className="text-[11px] font-medium text-foreground/70">Add observation</p>
        <div className="flex gap-2">
          {(["comment", "correction", "blocker", "praise"] as const).map((k) => (
            <button
              key={k}
              type="button"
              onClick={() => setFeedKind(k)}
              className={`rounded border px-2 py-0.5 font-mono text-[10px] transition-colors ${
                feedKind === k
                  ? "border-[#79b8ff]/50 bg-[#79b8ff]/10 text-[#79b8ff]"
                  : "border-admin-border text-muted-foreground hover:bg-white/5"
              }`}
            >
              {k}
            </button>
          ))}
          <Input
            className="h-7 w-20 border-admin-border bg-black/20 font-mono text-xs"
            placeholder="step #"
            value={feedStep}
            onChange={(e) => setFeedStep(e.target.value.replace(/\D/g, ""))}
          />
        </div>
        <Textarea
          className="min-h-[64px] border-admin-border bg-black/20 font-mono text-xs"
          placeholder="Describe what you observed…"
          value={feedBody}
          onChange={(e) => setFeedBody(e.target.value)}
        />
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant="outline"
            className="h-7 border-admin-border text-xs"
            disabled={!feedBody.trim() || addFeedback.isPending}
            onClick={() => addFeedback.mutate()}
          >
            {addFeedback.isPending ? "Saving…" : "Add"}
          </Button>
          {addFeedback.error && (
            <p className="text-[11px] text-destructive">
              {addFeedback.error instanceof Error
                ? addFeedback.error.message
                : "Failed"}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}

// ── Main panel ────────────────────────────────────────────────────────────────

export function AgentRunsPanel({
  apiBase,
  companyId,
  agentId,
}: {
  apiBase: string;
  companyId: string;
  agentId: string;
}) {
  const qc = useQueryClient();
  const [expanded, setExpanded] = useState<string | null>(null);
  const { data: selfImprove } = useSelfImprovementSummary(apiBase, companyId);

  const runsKey = ["hsm", "agent-runs", apiBase, companyId, agentId];

  const { data, isLoading, error } = useQuery({
    queryKey: runsKey,
    queryFn: async () => {
      const r = await fetch(
        `${apiBase}/api/company/companies/${companyId}/agent-runs?company_agent_id=${agentId}&limit=50`
      );
      if (!r.ok) throw new Error(`${r.status}`);
      return r.json() as Promise<{ runs: AgentRun[] }>;
    },
    refetchInterval: 20_000,
  });

  const invalidateRuns = () => void qc.invalidateQueries({ queryKey: runsKey });

  const runs = data?.runs ?? [];

  if (!companyId) {
    return (
      <p className="text-sm text-muted-foreground">Select a company to view runs.</p>
    );
  }

  return (
    <Card className="border-admin-border">
      <CardHeader className="pb-2">
        <CardTitle className="text-base">Agent runs</CardTitle>
        <CardDescription>
          Execution sessions for this agent. Expand a run to view the timeline, leave
          structured feedback, and promote observations to tasks.
        </CardDescription>
        <div className="mt-2 flex flex-wrap items-center gap-2 font-mono text-[10px] text-muted-foreground">
          <Badge variant="outline" className="text-[9px]">
            failure 7d: {selfImprove?.total_failures_7d ?? 0}
          </Badge>
          <Badge variant="outline" className="text-[9px]">
            first-pass: {Math.round((selfImprove?.first_pass_success_rate_7d ?? 1) * 100)}%
          </Badge>
          <Badge variant="outline" className="text-[9px]">
            applied fixes: {selfImprove?.proposals_applied_7d ?? 0}
          </Badge>
        </div>
      </CardHeader>
      <CardContent className="p-0">
        <ScrollArea className="h-[min(70vh,640px)]">
          <div className="min-w-[420px]">
            {/* Header */}
            <div className="pc-table-header grid-cols-[28px_1fr_90px_90px_100px]">
              <span />
              <span>Summary / run id</span>
              <span>Status</span>
              <span>Started</span>
              <span>System / Mode</span>
            </div>

            {isLoading ? (
              Array.from({ length: 5 }).map((_, i) => (
                <div key={i} className="flex gap-2 border-b border-admin-border px-3 py-3">
                  <Skeleton className="h-4 w-4" />
                  <Skeleton className="h-4 flex-1" />
                  <Skeleton className="h-4 w-20" />
                </div>
              ))
            ) : error ? (
              <p className="px-4 py-6 text-sm text-destructive">
                {error instanceof Error ? error.message : "Failed to load runs"}
              </p>
            ) : runs.length === 0 ? (
              <p className="px-4 py-8 text-sm text-muted-foreground">
                No runs recorded for this agent yet. Runs are created automatically when
                the agent starts working on a task, or manually via the API.
              </p>
            ) : (
              runs.map((run) => (
                <div key={run.id} className="border-b border-admin-border">
                  {/* Row */}
                  <button
                    type="button"
                    onClick={() => setExpanded(expanded === run.id ? null : run.id)}
                    className="pc-table-row grid w-full grid-cols-[28px_1fr_90px_90px_100px] gap-2 text-left hover:bg-white/3"
                  >
                    <span className="flex items-center justify-center text-muted-foreground">
                      {expanded === run.id ? (
                        <ChevronDown className="h-3 w-3" />
                      ) : (
                        <ChevronRight className="h-3 w-3" />
                      )}
                    </span>
                    <span className="min-w-0">
                      <p className="truncate text-xs text-foreground">
                        {run.summary ?? run.id}
                      </p>
                      {run.summary && (
                        <p className="font-mono text-[9px] text-muted-foreground">
                          {run.id}
                        </p>
                      )}
                    </span>
                    <span>
                      <Badge variant={statusVariant(run.status)} className="text-[9px]">
                        {run.status}
                      </Badge>
                    </span>
                    <span className="font-mono text-[10px] text-muted-foreground">
                      {fmtTs(run.started_at)}
                    </span>
                    <span className="font-mono text-[10px] text-muted-foreground">
                      {run.external_system}
                      <span className="block text-[9px] text-muted-foreground/80">
                        {executionMode(run)}
                      </span>
                    </span>
                  </button>

                  {/* Expanded detail */}
                  {expanded === run.id && (
                    <div className="border-t border-admin-border/50 bg-black/10 px-4">
                      <RunDetail
                        apiBase={apiBase}
                        companyId={companyId}
                        run={run}
                        onPromoted={invalidateRuns}
                      />
                    </div>
                  )}
                </div>
              ))
            )}
          </div>
        </ScrollArea>
      </CardContent>
    </Card>
  );
}
