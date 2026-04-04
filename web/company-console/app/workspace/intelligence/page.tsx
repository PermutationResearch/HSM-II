"use client";

import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/app/components/ui/card";
import { ScrollArea } from "@/app/components/ui/scroll-area";
import { Skeleton } from "@/app/components/ui/skeleton";
import { Badge } from "@/app/components/ui/badge";
import { Button } from "@/app/components/ui/button";
import { Input } from "@/app/components/ui/input";
import { cn } from "@/app/lib/utils";

// ── Types ──────────────────────────────────────────────────────────────────

type PaperclipSummary = {
  total_goals: number;
  open_goals: number;
  in_progress_goals: number;
  done_goals: number;
  total_capabilities: number;
  healthy_capabilities: number;
  total_dris: number;
  queued_signals: number;
};

type Goal = {
  id: string;
  title: string;
  status: string;
  assignee_type: string;
  assignee_id: string | null;
  parent_goal_id: string | null;
  created_at: string;
};

type Capability = {
  id: string;
  name: string;
  domain: string;
  health_score: number;
  reliability_target: number;
  cost_per_run_cents: number | null;
};

type DriEntry = {
  id: string;
  name: string;
  domain: string;
  authority_scope: string[];
  budget_authority_cents: number | null;
};

// ── Fetchers ───────────────────────────────────────────────────────────────

const API = "/api/paperclip";

async function fetchJson<T>(path: string): Promise<T> {
  const r = await fetch(`${API}/${path}`);
  if (!r.ok) throw new Error(`${r.status} ${r.statusText}`);
  return r.json() as Promise<T>;
}

async function postJson<T>(path: string, body: unknown): Promise<T> {
  const r = await fetch(`${API}/${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!r.ok) throw new Error(`${r.status} ${r.statusText}`);
  return r.json() as Promise<T>;
}

// ── Tier badge helpers ─────────────────────────────────────────────────────

function healthColor(score: number) {
  if (score >= 0.9) return "text-emerald-400";
  if (score >= 0.7) return "text-yellow-400";
  return "text-red-400";
}

function statusBadgeVariant(status: string): "default" | "secondary" | "destructive" | "outline" {
  if (status === "done") return "default";
  if (status === "in_progress") return "secondary";
  if (status === "blocked" || status === "escalated") return "destructive";
  return "outline";
}

// ── Page ──────────────────────────────────────────────────────────────────

export default function IntelligencePage() {
  const qc = useQueryClient();

  const { data: summary, isLoading: sumLoading } = useQuery<PaperclipSummary>({
    queryKey: ["paperclip", "summary"],
    queryFn: () => fetchJson("summary"),
    refetchInterval: 15_000,
  });

  const { data: goals = [], isLoading: goalsLoading, error: goalsError } = useQuery<Goal[]>({
    queryKey: ["paperclip", "goals"],
    queryFn: () => fetchJson("goals"),
    refetchInterval: 15_000,
  });

  const { data: capabilities = [], isLoading: capsLoading } = useQuery<Capability[]>({
    queryKey: ["paperclip", "capabilities"],
    queryFn: () => fetchJson("capabilities"),
    refetchInterval: 30_000,
  });

  const { data: dris = [], isLoading: drisLoading } = useQuery<DriEntry[]>({
    queryKey: ["paperclip", "dris"],
    queryFn: () => fetchJson("dris"),
    refetchInterval: 30_000,
  });

  // ── Create goal ──
  const [newGoalTitle, setNewGoalTitle] = useState("");
  const createGoal = useMutation({
    mutationFn: (title: string) => postJson("goals", { title, assignee_type: "Unassigned" }),
    onSuccess: () => {
      setNewGoalTitle("");
      void qc.invalidateQueries({ queryKey: ["paperclip"] });
    },
  });

  // ── Emit signal ──
  const [sigType, setSigType] = useState("ExternalSignal");
  const [sigPayload, setSigPayload] = useState("");
  const emitSignal = useMutation({
    mutationFn: () => postJson("signals", { signal_type: sigType, payload: sigPayload || null }),
    onSuccess: () => {
      setSigPayload("");
      void qc.invalidateQueries({ queryKey: ["paperclip", "summary"] });
    },
  });

  // ── Summary stat cards ──
  const stats = summary
    ? [
        { label: "Open goals", value: summary.open_goals },
        { label: "In progress", value: summary.in_progress_goals },
        { label: "Done", value: summary.done_goals },
        { label: "Capabilities", value: `${summary.healthy_capabilities}/${summary.total_capabilities}` },
        { label: "DRIs", value: summary.total_dris },
        { label: "Queued signals", value: summary.queued_signals },
      ]
    : [];

  return (
    <div className="space-y-6">
      {/* Header */}
      <div>
        <p className="pc-page-eyebrow">Runtime</p>
        <h1 className="pc-page-title">Intelligence Layer</h1>
        <p className="pc-page-desc">
          Goals, capabilities, DRI registry, and signal queue from{" "}
          <span className="font-mono text-xs">/api/paperclip/*</span> — shared across all companies.
        </p>
      </div>

      {/* Summary stats */}
      <div className="grid grid-cols-3 gap-3 sm:grid-cols-6">
        {sumLoading
          ? Array.from({ length: 6 }).map((_, i) => <Skeleton key={i} className="h-16 rounded-lg" />)
          : stats.map((s) => (
              <Card key={s.label} className="pc-panel border-admin-border">
                <CardContent className="px-3 py-3">
                  <p className="font-mono text-2xl tabular-nums text-foreground">{s.value}</p>
                  <p className="mt-0.5 text-[11px] text-muted-foreground">{s.label}</p>
                </CardContent>
              </Card>
            ))}
      </div>

      {/* Goals + create */}
      <Card className="pc-panel border-admin-border">
        <CardHeader className="pb-2">
          <CardTitle className="text-base">Goals</CardTitle>
          <CardDescription>Managed by the Intelligence Layer — auto-routed via DRI registry</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3 p-0">
          {/* create form */}
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

          <ScrollArea className="h-[min(40vh,320px)]">
            <div className="min-w-[480px]">
              <div className="pc-table-header grid-cols-[1fr_100px_120px_80px]">
                <span>Title</span>
                <span>Status</span>
                <span>Assignee</span>
                <span>Action</span>
              </div>
              {goalsLoading
                ? Array.from({ length: 5 }).map((_, i) => (
                    <div key={i} className="flex gap-2 border-b border-admin-border px-4 py-3">
                      <Skeleton className="h-4 flex-1" />
                      <Skeleton className="h-4 w-20" />
                    </div>
                  ))
                : goals.length === 0
                  ? <p className="px-4 py-6 text-sm text-muted-foreground">No goals yet. Create one above.</p>
                  : goals.map((g) => (
                      <div key={g.id} className="pc-table-row grid grid-cols-[1fr_100px_120px_80px] gap-2">
                        <span className="truncate text-xs text-foreground">{g.title}</span>
                        <span>
                          <Badge variant={statusBadgeVariant(g.status)} className="text-[10px]">
                            {g.status}
                          </Badge>
                        </span>
                        <span className="truncate font-mono text-[11px] text-muted-foreground">
                          {g.assignee_type}{g.assignee_id ? `:${g.assignee_id.slice(0, 6)}` : ""}
                        </span>
                        <span>
                          {g.status !== "done" && g.status !== "cancelled" ? (
                            <CompleteGoalButton
                              goalId={g.id}
                              onDone={() => qc.invalidateQueries({ queryKey: ["paperclip"] })}
                            />
                          ) : null}
                        </span>
                      </div>
                    ))}
            </div>
          </ScrollArea>
        </CardContent>
      </Card>

      <div className="grid gap-4 md:grid-cols-2">
        {/* Capabilities */}
        <Card className="pc-panel border-admin-border">
          <CardHeader className="pb-2">
            <CardTitle className="text-base">Capabilities</CardTitle>
            <CardDescription>Atomic primitives with reliability + health scoring</CardDescription>
          </CardHeader>
          <CardContent className="p-0">
            <ScrollArea className="h-[min(40vh,280px)]">
              <div className="min-w-[340px]">
                <div className="pc-table-header grid-cols-[1fr_80px_60px]">
                  <span>Name</span>
                  <span>Domain</span>
                  <span>Health</span>
                </div>
                {capsLoading
                  ? Array.from({ length: 6 }).map((_, i) => (
                      <div key={i} className="flex gap-2 border-b border-admin-border px-4 py-3">
                        <Skeleton className="h-4 flex-1" />
                        <Skeleton className="h-4 w-16" />
                      </div>
                    ))
                  : capabilities.length === 0
                    ? <p className="px-4 py-6 text-sm text-muted-foreground">No capabilities registered.</p>
                    : capabilities.map((c) => (
                        <div key={c.id} className="pc-table-row grid grid-cols-[1fr_80px_60px] gap-2">
                          <span className="truncate text-xs text-foreground">{c.name}</span>
                          <span className="truncate font-mono text-[11px] text-muted-foreground">{c.domain}</span>
                          <span className={cn("font-mono text-xs tabular-nums", healthColor(c.health_score))}>
                            {Math.round(c.health_score * 100)}%
                          </span>
                        </div>
                      ))}
              </div>
            </ScrollArea>
          </CardContent>
        </Card>

        {/* DRI Registry */}
        <Card className="pc-panel border-admin-border">
          <CardHeader className="pb-2">
            <CardTitle className="text-base">DRI Registry</CardTitle>
            <CardDescription>Domain owners with explicit authority scope</CardDescription>
          </CardHeader>
          <CardContent className="p-0">
            <ScrollArea className="h-[min(40vh,280px)]">
              <div className="min-w-[340px]">
                <div className="pc-table-header grid-cols-[1fr_1fr_80px]">
                  <span>Name</span>
                  <span>Domain</span>
                  <span>Authority</span>
                </div>
                {drisLoading
                  ? Array.from({ length: 4 }).map((_, i) => (
                      <div key={i} className="flex gap-2 border-b border-admin-border px-4 py-3">
                        <Skeleton className="h-4 flex-1" />
                        <Skeleton className="h-4 w-24" />
                      </div>
                    ))
                  : dris.length === 0
                    ? <p className="px-4 py-6 text-sm text-muted-foreground">No DRIs registered.</p>
                    : dris.map((d) => (
                        <div key={d.id} className="pc-table-row grid grid-cols-[1fr_1fr_80px] gap-2">
                          <span className="truncate text-xs text-foreground">{d.name}</span>
                          <span className="truncate font-mono text-[11px] text-muted-foreground">{d.domain}</span>
                          <span className="font-mono text-[11px] text-muted-foreground">
                            {d.authority_scope.length} perms
                          </span>
                        </div>
                      ))}
              </div>
            </ScrollArea>
          </CardContent>
        </Card>
      </div>

      {/* Signal emitter */}
      <Card className="pc-panel border-admin-border">
        <CardHeader className="pb-2">
          <CardTitle className="text-base">Emit Signal</CardTitle>
          <CardDescription>
            Inject a signal into the Intelligence Layer — triggers composition engine on next tick (every 60 s)
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex flex-wrap gap-2">
            <select
              className="h-8 rounded-md border border-admin-border bg-black/20 px-2 font-mono text-xs text-foreground"
              value={sigType}
              onChange={(e) => setSigType(e.target.value)}
            >
              {[
                "ExternalSignal",
                "CapabilityDegraded",
                "GoalStale",
                "BudgetOverrun",
                "CompositionFailed",
                "MissingCapability",
                "CoherenceDrop",
                "AgentAnomaly",
                "Custom",
              ].map((t) => (
                <option key={t} value={t}>{t}</option>
              ))}
            </select>
            <Input
              className="h-8 w-64 border-admin-border bg-black/20 font-mono text-xs"
              placeholder="Optional payload…"
              value={sigPayload}
              onChange={(e) => setSigPayload(e.target.value)}
            />
            <Button
              size="sm"
              variant="outline"
              className="border-admin-border text-xs"
              disabled={emitSignal.isPending}
              onClick={() => emitSignal.mutate()}
            >
              {emitSignal.isPending ? "Emitting…" : "Emit"}
            </Button>
            {emitSignal.isSuccess ? (
              <span className="self-center text-xs text-emerald-400">Signal queued ✓</span>
            ) : null}
            {emitSignal.isError ? (
              <span className="self-center text-xs text-destructive">
                {emitSignal.error instanceof Error ? emitSignal.error.message : "Error"}
              </span>
            ) : null}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

// ── Inline complete button ─────────────────────────────────────────────────

function CompleteGoalButton({ goalId, onDone }: { goalId: string; onDone: () => void }) {
  const mut = useMutation({
    mutationFn: () => postJson(`goals/${goalId}/complete`, {}),
    onSuccess: onDone,
  });
  return (
    <button
      className="rounded border border-admin-border px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground hover:bg-white/5 hover:text-foreground disabled:opacity-40"
      disabled={mut.isPending}
      onClick={() => mut.mutate()}
    >
      {mut.isPending ? "…" : "Done"}
    </button>
  );
}
