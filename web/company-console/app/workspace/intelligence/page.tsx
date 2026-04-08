"use client";

import Link from "next/link";
import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/app/components/ui/card";
import { ScrollArea } from "@/app/components/ui/scroll-area";
import { Skeleton } from "@/app/components/ui/skeleton";
import { Badge } from "@/app/components/ui/badge";
import { Button } from "@/app/components/ui/button";
import { Input } from "@/app/components/ui/input";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import {
  useCompanyGoals,
  useCompanyIntelligenceSummary,
} from "@/app/lib/hsm-queries";
import type { HsmGoalRow, HsmWorkflowFeedEvent } from "@/app/lib/hsm-api-types";

function statusBadgeVariant(status: string): "default" | "secondary" | "destructive" | "outline" {
  const s = status.toLowerCase();
  if (s === "done" || s === "closed" || s === "completed") return "default";
  if (s === "active" || s === "open" || s === "in_progress") return "secondary";
  if (s === "blocked" || s === "escalated" || s === "cancelled") return "destructive";
  return "outline";
}

function goalIsTerminal(status: string): boolean {
  const s = status.toLowerCase();
  return s === "done" || s === "closed" || s === "completed" || s === "cancelled";
}

function formatUsd(n: number): string {
  if (!Number.isFinite(n)) return "—";
  return new Intl.NumberFormat(undefined, { style: "currency", currency: "USD", maximumFractionDigits: 2 }).format(n);
}

function payloadPreview(p: unknown, max = 120): string {
  try {
    const s = JSON.stringify(p ?? {});
    return s.length <= max ? s : `${s.slice(0, max)}…`;
  } catch {
    return "…";
  }
}

function actionLabel(action: string): string {
  const map: Record<string, string> = {
    task_created: "Task created",
    task_checkout_agent_profile: "Checkout",
    release_checkout: "Released",
    task_requires_human: "Requires human",
    task_spawn_subagents: "Spawned subagents",
    task_policy_decision: "Policy",
    task_run_terminal: "Run finished",
    task_capability_refs_updated: "Capabilities linked",
    paperclip_goals_synced: "Paperclip → Postgres goals",
    paperclip_dris_synced: "Paperclip → Postgres DRIs",
  };
  return map[action] ?? action;
}

// ── Page ──────────────────────────────────────────────────────────────────

export default function IntelligencePage() {
  const qc = useQueryClient();
  const { apiBase, companyId, companies } = useWorkspace();
  const companyLabel = companies.find((c) => c.id === companyId)?.display_name ?? null;

  const { data: intel, isLoading: intelLoading, error: intelError } = useCompanyIntelligenceSummary(
    apiBase,
    companyId,
  );

  const { data: goals = [], isLoading: goalsLoading, error: goalsError } = useCompanyGoals(apiBase, companyId);

  const [newGoalTitle, setNewGoalTitle] = useState("");

  const invalidateIntel = () => {
    void qc.invalidateQueries({ queryKey: ["hsm", "intelligence", apiBase, companyId] });
    void qc.invalidateQueries({ queryKey: ["hsm", "goals", apiBase, companyId] });
  };

  const createGoal = useMutation({
    mutationFn: async (title: string) => {
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/goals`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ title: title.trim() }),
      });
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    onSuccess: () => {
      setNewGoalTitle("");
      invalidateIntel();
    },
  });

  const stats = intel
    ? [
        { label: "Goals (active)", value: intel.goals.active },
        { label: "Goals (total)", value: intel.goals.total },
        { label: "Tasks open", value: intel.tasks.open },
        { label: "In progress", value: intel.tasks.in_progress },
        { label: "Done / closed", value: intel.tasks.done_or_closed },
        { label: "Needs human", value: intel.tasks.requires_human_open },
        { label: "Checked out now", value: intel.tasks.checked_out_now },
        { label: "Agents", value: intel.workforce.agents_non_terminated },
        { label: "Spend (total)", value: formatUsd(intel.spend.total_usd) },
      ]
    : [];

  if (!companyId) {
    return (
      <div className="space-y-4">
        <p className="pc-page-eyebrow">Runtime</p>
        <h1 className="pc-page-title">Intelligence</h1>
        <p className="pc-page-desc">
          Select a company to load the runtime view: Postgres-backed goals, tasks, and workflow signals—the console
          face of company coordination, not the full composer or hypergraph.
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div>
        <p className="pc-page-eyebrow">Runtime</p>
        <h1 className="pc-page-title">Intelligence</h1>
        <p className="pc-page-desc">
          This page helps represent the intelligence layer in the product: a{" "}
          <span className="text-foreground/90">company-scoped runtime view</span> of goals, task
          states, spend, workforce, and workflow events from Postgres—live state and provenance-friendly
          signals that ground alignment and routing. It is not the whole unified world model or dynamic
          composer; those live across HyperStigmergicMorphogenesis, capabilities, gateways, and jobs.
          Canonical API{" "}
          <span className="font-mono text-xs">
            GET /api/company/companies/{`{id}`}/intelligence/summary
          </span>
          {companyLabel ? (
            <>
              {" "}
              — <span className="text-foreground/90">{companyLabel}</span>
            </>
          ) : null}
          .
        </p>
        <p className="mt-2 text-[11px] text-muted-foreground">
          Legacy demo layer:{" "}
          <Link href="/api/paperclip/summary" className="font-mono text-[#79b8ff] underline-offset-2 hover:underline">
            /api/paperclip/*
          </Link>{" "}
          (global, not company-scoped).
        </p>
      </div>

      {intelError ? (
        <p className="text-xs text-destructive">
          {intelError instanceof Error ? intelError.message : "Failed to load intelligence summary"}
        </p>
      ) : null}

      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-5">
        {intelLoading
          ? Array.from({ length: 9 }).map((_, i) => <Skeleton key={i} className="h-16 rounded-lg" />)
          : stats.map((s) => (
              <Card key={s.label} className="pc-panel border-admin-border">
                <CardContent className="px-3 py-3">
                  <p className="font-mono text-xl tabular-nums text-foreground">{s.value}</p>
                  <p className="mt-0.5 text-[11px] text-muted-foreground">{s.label}</p>
                </CardContent>
              </Card>
            ))}
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <Card className="pc-panel border-admin-border lg:col-span-1">
          <CardHeader className="pb-2">
            <CardTitle className="text-base">Workflow feed</CardTitle>
            <CardDescription>
              Recent signals from task lifecycle (create, checkout, human, spawn, policy, terminal runs)
            </CardDescription>
          </CardHeader>
          <CardContent className="p-0">
            <ScrollArea className="h-[min(48vh,420px)]">
              <div className="min-w-[360px] px-1">
                <div className="pc-table-header grid-cols-[120px_1fr_100px]">
                  <span>When</span>
                  <span>Event</span>
                  <span>Subject</span>
                </div>
                {intelLoading ? (
                  Array.from({ length: 6 }).map((_, i) => (
                    <div key={i} className="flex gap-2 border-b border-admin-border px-3 py-3">
                      <Skeleton className="h-4 w-24" />
                      <Skeleton className="h-4 flex-1" />
                    </div>
                  ))
                ) : !intel?.workflow_feed?.length ? (
                  <p className="px-4 py-6 text-sm text-muted-foreground">No workflow events yet for this company.</p>
                ) : (
                  intel.workflow_feed.map((e: HsmWorkflowFeedEvent) => (
                    <div
                      key={e.id}
                      className="pc-table-row grid grid-cols-[120px_1fr_100px] gap-2 border-b border-admin-border"
                    >
                      <span className="truncate font-mono text-[10px] text-muted-foreground">{e.created_at}</span>
                      <span className="min-w-0">
                        <Badge variant="outline" className="mr-1 text-[10px]">
                          {actionLabel(e.action)}
                        </Badge>
                        <span className="font-mono text-[10px] text-muted-foreground">{e.actor}</span>
                        <p className="mt-0.5 truncate font-mono text-[9px] text-muted-foreground/80">
                          {payloadPreview(e.payload ?? {})}
                        </p>
                      </span>
                      <span className="truncate font-mono text-[10px] text-muted-foreground" title={e.subject_id}>
                        {e.subject_type}:
                        {e.subject_id.length > 10 ? `${e.subject_id.slice(0, 8)}…` : e.subject_id}
                      </span>
                    </div>
                  ))
                )}
              </div>
            </ScrollArea>
          </CardContent>
        </Card>

        <Card className="pc-panel border-admin-border">
          <CardHeader className="pb-2">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div>
                <CardTitle className="text-base">Goals</CardTitle>
                <CardDescription>Postgres spine — same company as tasks and spend</CardDescription>
              </div>
              <Button asChild size="sm" variant="ghost" className="h-7 text-[11px] text-[#79b8ff]">
                <Link href="/workspace/issues">Open tasks →</Link>
              </Button>
            </div>
          </CardHeader>
          <CardContent className="space-y-3 p-0">
            <div className="flex gap-2 px-4 pb-2 pt-1">
              <Input
                className="h-8 border-admin-border bg-black/20 font-mono text-xs"
                placeholder="New goal title…"
                value={newGoalTitle}
                onChange={(e) => setNewGoalTitle(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && newGoalTitle.trim()) {
                    createGoal.mutate(newGoalTitle.trim());
                  }
                }}
              />
              <Button
                size="sm"
                variant="outline"
                className="shrink-0 border-admin-border text-xs"
                disabled={!newGoalTitle.trim() || createGoal.isPending}
                onClick={() => createGoal.mutate(newGoalTitle.trim())}
              >
                {createGoal.isPending ? "…" : "Add"}
              </Button>
            </div>

            {goalsError ? (
              <p className="px-4 pb-3 text-xs text-destructive">
                {goalsError instanceof Error ? goalsError.message : "Failed to load goals"}
              </p>
            ) : null}

            <ScrollArea className="h-[min(48vh,420px)]">
              <div className="min-w-[400px]">
                <div className="pc-table-header grid-cols-[1fr_100px_80px]">
                  <span>Title</span>
                  <span>Status</span>
                  <span>Action</span>
                </div>
                {goalsLoading
                  ? Array.from({ length: 5 }).map((_, i) => (
                      <div key={i} className="flex gap-2 border-b border-admin-border px-4 py-3">
                        <Skeleton className="h-4 flex-1" />
                        <Skeleton className="h-4 w-20" />
                      </div>
                    ))
                  : goals.length === 0 ? (
                      <p className="px-4 py-6 text-sm text-muted-foreground">No goals yet. Create one above.</p>
                    ) : (
                      goals.map((g: HsmGoalRow) => (
                        <div key={g.id} className="pc-table-row grid grid-cols-[1fr_100px_80px] gap-2">
                          <span className="truncate text-xs text-foreground">{g.title}</span>
                          <span>
                            <Badge variant={statusBadgeVariant(g.status)} className="text-[10px]">
                              {g.status}
                            </Badge>
                          </span>
                          <span>
                            {!goalIsTerminal(g.status) ? (
                              <CompleteGoalButton apiBase={apiBase} companyId={companyId} goalId={g.id} onDone={invalidateIntel} />
                            ) : null}
                          </span>
                        </div>
                      ))
                    )}
              </div>
            </ScrollArea>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

function CompleteGoalButton({
  apiBase,
  companyId,
  goalId,
  onDone,
}: {
  apiBase: string;
  companyId: string;
  goalId: string;
  onDone: () => void;
}) {
  const mut = useMutation({
    mutationFn: async () => {
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/goals/${goalId}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ status: "done" }),
      });
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    onSuccess: onDone,
  });
  return (
    <button
      type="button"
      className="rounded border border-admin-border px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground hover:bg-white/5 hover:text-foreground disabled:opacity-40"
      disabled={mut.isPending}
      onClick={() => mut.mutate()}
    >
      {mut.isPending ? "…" : "Done"}
    </button>
  );
}
