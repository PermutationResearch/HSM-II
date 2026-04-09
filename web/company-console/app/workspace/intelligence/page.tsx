"use client";

import Link from "next/link";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/app/components/ui/card";
import { ScrollArea } from "@/app/components/ui/scroll-area";
import { Skeleton } from "@/app/components/ui/skeleton";
import { Badge } from "@/app/components/ui/badge";
import { Button } from "@/app/components/ui/button";
import { Input } from "@/app/components/ui/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/app/components/ui/tabs";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import {
  useCompanyGoals,
  useCompanyIntelligenceSummary,
  useMemoryArtifacts,
  useMemoryInspect,
  useSelfImprovementProposals,
  useSelfImprovementSummary,
  useStorePromotions,
} from "@/app/lib/hsm-queries";
import type {
  HsmGoalRow,
  HsmMemoryArtifact,
  HsmMemoryInspect,
  HsmMemoryMatch,
  HsmSignalRow,
  HsmWorkflowFeedEvent,
} from "@/app/lib/hsm-api-types";

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

// ── Signal helpers ────────────────────────────────────────────────────────

function signalKindVariant(kind: string): "default" | "secondary" | "destructive" | "outline" {
  if (kind === "coherence_drop" || kind === "missing_capability") return "destructive";
  if (kind === "budget_overrun" || kind === "agent_anomaly") return "outline";
  if (kind === "capability_degraded" || kind === "composition_failed") return "outline";
  return "secondary";
}

function severityBar(severity: number): string {
  if (severity >= 0.8) return "bg-destructive";
  if (severity >= 0.6) return "bg-yellow-500";
  return "bg-muted-foreground/40";
}

function splitTagList(input: string): string[] {
  return input
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
}

function tryParseArray(input: string): { items: unknown[] | null; error: string | null } {
  if (!input.trim()) return { items: null, error: null };
  try {
    const parsed = JSON.parse(input);
    if (!Array.isArray(parsed)) {
      return { items: null, error: "Expected a JSON array." };
    }
    return { items: parsed, error: null };
  } catch {
    return { items: null, error: "Invalid JSON." };
  }
}

function SignalFeed({ apiBase, companyId }: { apiBase: string; companyId: string }) {
  const { data, isLoading, error } = useQuery({
    queryKey: ["hsm", "signals", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(
        `${apiBase}/api/company/companies/${companyId}/intelligence/signals?limit=60`
      );
      if (!r.ok) throw new Error(`${r.status}`);
      return r.json() as Promise<{ signals: HsmSignalRow[] }>;
    },
    refetchInterval: 30_000,
    enabled: !!companyId,
  });

  const signals = data?.signals ?? [];

  return (
    <Card className="pc-panel border-admin-border">
      <CardHeader className="pb-2">
        <CardTitle className="text-base">Signal log</CardTitle>
        <CardDescription>
          Intelligence Layer detections — capability health, goal staleness, budget
          anomalies, inbound from Company OS. Each signal drives a composition attempt.
        </CardDescription>
      </CardHeader>
      <CardContent className="p-0">
        <ScrollArea className="h-[min(48vh,420px)]">
          <div className="min-w-[380px] px-1">
            <div className="pc-table-header grid-cols-[6px_120px_1fr_70px]">
              <span />
              <span>Kind</span>
              <span>Description</span>
              <span>Severity</span>
            </div>
            {isLoading ? (
              Array.from({ length: 6 }).map((_, i) => (
                <div key={i} className="flex gap-2 border-b border-admin-border px-3 py-3">
                  <Skeleton className="h-4 w-24" />
                  <Skeleton className="h-4 flex-1" />
                </div>
              ))
            ) : error ? (
              <p className="px-4 py-6 text-xs text-destructive">
                {error instanceof Error ? error.message : "Failed to load signals"}
              </p>
            ) : signals.length === 0 ? (
              <p className="px-4 py-6 text-sm text-muted-foreground">
                No signals yet. They appear once the Intelligence Layer runs a tick with
                Company OS data attached.
              </p>
            ) : (
              signals.map((s: HsmSignalRow) => (
                <div
                  key={s.id}
                  className="pc-table-row grid grid-cols-[6px_120px_1fr_70px] gap-2 border-b border-admin-border"
                >
                  <span
                    className={`mt-1.5 h-1.5 w-1.5 shrink-0 self-start rounded-full ${severityBar(s.severity)}`}
                  />
                  <span>
                    <Badge variant={signalKindVariant(s.kind)} className="text-[9px]">
                      {s.kind.replace(/_/g, " ")}
                    </Badge>
                    <p className="mt-0.5 font-mono text-[9px] text-muted-foreground">
                      {s.created_at}
                    </p>
                  </span>
                  <span className="min-w-0">
                    <p className="truncate text-[11px] text-foreground/90">{s.description}</p>
                    {s.escalated_to && (
                      <p className="font-mono text-[9px] text-muted-foreground">
                        → {s.escalated_to}
                      </p>
                    )}
                  </span>
                  <span className="flex items-center gap-1">
                    <div
                      className={`h-1.5 rounded-full ${severityBar(s.severity)}`}
                      style={{ width: `${Math.round(s.severity * 48)}px` }}
                    />
                    <span className="font-mono text-[9px] text-muted-foreground">
                      {(s.severity * 100).toFixed(0)}%
                    </span>
                  </span>
                </div>
              ))
            )}
          </div>
        </ScrollArea>
      </CardContent>
    </Card>
  );
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
  const { data: selfImprove } = useSelfImprovementSummary(apiBase, companyId);
  const { data: proposals = [], isLoading: proposalsLoading, error: proposalsError } =
    useSelfImprovementProposals(apiBase, companyId);
  const { data: promotions = [], isLoading: promotionsLoading } =
    useStorePromotions(apiBase, companyId);
  const { data: artifacts = [], isLoading: artifactsLoading } =
    useMemoryArtifacts(apiBase, companyId);

  const [newGoalTitle, setNewGoalTitle] = useState("");
  const [skillDraft, setSkillDraft] = useState({
    skill_id: "",
    title: "",
    principle: "",
    role: "",
    task: "",
  });
  const [beliefDraft, setBeliefDraft] = useState({
    title: "",
    content: "",
    confidence: "0.8",
    tags: "skill",
  });
  const [roodbJsonInput, setRoodbJsonInput] = useState("");
  const [ladybugJsonInput, setLadybugJsonInput] = useState("");
  const [confirmingRollback, setConfirmingRollback] = useState<string | null>(null);
  const [confirmingApply, setConfirmingApply] = useState<string | null>(null);
  const [webIngestUrl, setWebIngestUrl] = useState("");
  const [fileIngestPath, setFileIngestPath] = useState("");
  const [imageIngestText, setImageIngestText] = useState("");
  const [audioIngestText, setAudioIngestText] = useState("");
  const [retrievalDraft, setRetrievalDraft] = useState("");
  const [retrievalApplied, setRetrievalApplied] = useState("");
  const [memoryEntityType, setMemoryEntityType] = useState("");
  const [memoryEntityId, setMemoryEntityId] = useState("");
  const [memoryLatestOnly, setMemoryLatestOnly] = useState(true);
  const [selectedMemoryId, setSelectedMemoryId] = useState<string | null>(null);

  const invalidateIntel = () => {
    void qc.invalidateQueries({ queryKey: ["hsm", "intelligence", apiBase, companyId] });
    void qc.invalidateQueries({ queryKey: ["hsm", "goals", apiBase, companyId] });
  };
  const invalidateMemory = () => {
    void qc.invalidateQueries({ queryKey: ["hsm", "memory-artifacts", apiBase, companyId] });
    void qc.invalidateQueries({ queryKey: ["hsm", "memory-inspect", apiBase, companyId] });
    void qc.invalidateQueries({ queryKey: ["hsm", "memory-metrics", apiBase, companyId] });
  };

  const { data: memoryInspect } = useMemoryInspect(apiBase, companyId, selectedMemoryId);
  const retrievalDebug = useQuery({
    queryKey: [
      "hsm",
      "memory-retrieval-debug",
      apiBase,
      companyId,
      retrievalApplied,
      memoryEntityType,
      memoryEntityId,
      memoryLatestOnly,
    ],
    queryFn: async () => {
      const qs = new URLSearchParams();
      qs.set("q", retrievalApplied);
      qs.set("scope", "shared");
      if (memoryLatestOnly) qs.set("latest_only", "true");
      if (memoryEntityType.trim()) qs.set("entity_type", memoryEntityType.trim());
      if (memoryEntityId.trim()) qs.set("entity_id", memoryEntityId.trim());
      const r = await fetch(
        `${apiBase}/api/company/companies/${companyId}/memory/retrieval-debug?${qs.toString()}`,
      );
      const j = (await r.json().catch(() => ({}))) as {
        matches?: HsmMemoryMatch[];
        meta?: Record<string, unknown>;
        error?: string;
      };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    enabled: !!companyId && retrievalApplied.trim().length > 0,
  });
  const memoryMetrics = useQuery({
    queryKey: ["hsm", "memory-metrics", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/memory/metrics`);
      const j = (await r.json().catch(() => ({}))) as {
        artifact_status_counts?: Array<{ status: string; count: number }>;
        chunk_embeddings_ready?: number;
        error?: string;
      };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    enabled: !!companyId,
    refetchInterval: 15_000,
  });

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

  const generateProposals = useMutation({
    mutationFn: async () => {
      const r = await fetch(
        `${apiBase}/api/company/companies/${companyId}/self-improvement/proposals/generate?limit=24`,
        { method: "POST" },
      );
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["hsm", "self-improvement", apiBase, companyId] });
      void qc.invalidateQueries({ queryKey: ["hsm", "intelligence", apiBase, companyId] });
    },
  });

  const runWeeklyNudge = useMutation({
    mutationFn: async () => {
      const r = await fetch(`${apiBase}/api/company/self-improvement/weekly-nudge`, { method: "POST" });
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["hsm", "self-improvement", apiBase, companyId] });
      void qc.invalidateQueries({ queryKey: ["hsm", "intelligence", apiBase, companyId] });
    },
  });

  const replayProposal = useMutation({
    mutationFn: async (proposalId: string) => {
      const r = await fetch(
        `${apiBase}/api/company/companies/${companyId}/self-improvement/proposals/${proposalId}/replay`,
        { method: "POST" },
      );
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["hsm", "self-improvement-proposals", apiBase, companyId] });
      void qc.invalidateQueries({ queryKey: ["hsm", "self-improvement", apiBase, companyId] });
    },
  });

  const applyProposal = useMutation({
    mutationFn: async (proposalId: string) => {
      const r = await fetch(
        `${apiBase}/api/company/companies/${companyId}/self-improvement/proposals/${proposalId}/apply`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ approved_by: "operator-ui" }),
        },
      );
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["hsm", "self-improvement-proposals", apiBase, companyId] });
      void qc.invalidateQueries({ queryKey: ["hsm", "self-improvement", apiBase, companyId] });
      invalidateIntel();
    },
  });

  const promoteRoodb = useMutation({
    mutationFn: async (skills: unknown[]) => {
      const r = await fetch(
        `${apiBase}/api/company/companies/${companyId}/promote/roodb-skills`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ skills, promoted_by: "operator-ui" }),
        },
      );
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    onSuccess: () => {
      setSkillDraft({
        skill_id: "",
        title: "",
        principle: "",
        role: "",
        task: "",
      });
      setRoodbJsonInput("");
      void qc.invalidateQueries({ queryKey: ["hsm", "store-promotions", apiBase, companyId] });
    },
  });

  const importLadybug = useMutation({
    mutationFn: async (beliefs: unknown[]) => {
      const r = await fetch(
        `${apiBase}/api/company/companies/${companyId}/promote/ladybug-bundle`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ beliefs, promoted_by: "operator-ui" }),
        },
      );
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    onSuccess: () => {
      setBeliefDraft({
        title: "",
        content: "",
        confidence: "0.8",
        tags: "skill",
      });
      setLadybugJsonInput("");
      void qc.invalidateQueries({ queryKey: ["hsm", "store-promotions", apiBase, companyId] });
    },
  });

  const rollbackPromotion = useMutation({
    mutationFn: async (promotionId: string) => {
      const r = await fetch(
        `${apiBase}/api/company/companies/${companyId}/promote/rollback/${promotionId}`,
        { method: "POST" },
      );
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["hsm", "store-promotions", apiBase, companyId] });
    },
  });

  const ingestMemory = useMutation({
    mutationFn: async ({
      endpoint,
      body,
    }: {
      endpoint: "web" | "file" | "image" | "audio";
      body: Record<string, unknown>;
    }) => {
      const r = await fetch(
        `${apiBase}/api/company/companies/${companyId}/memory/ingest/${endpoint}`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(body),
        },
      );
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    onSuccess: () => {
      setWebIngestUrl("");
      setFileIngestPath("");
      setImageIngestText("");
      setAudioIngestText("");
      invalidateMemory();
    },
  });

  const retryArtifact = useMutation({
    mutationFn: async (artifactId: string) => {
      const r = await fetch(
        `${apiBase}/api/company/companies/${companyId}/memory/artifacts/${artifactId}/retry`,
        { method: "POST" },
      );
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    onSuccess: () => invalidateMemory(),
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

  const parsedSkillBulk = tryParseArray(roodbJsonInput);
  const parsedBeliefBulk = tryParseArray(ladybugJsonInput);
  const skillDraftReady =
    skillDraft.skill_id.trim() && skillDraft.title.trim() && skillDraft.principle.trim();
  const beliefDraftReady = beliefDraft.content.trim();

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
    <div className="space-y-5">
      <div>
        <p className="pc-page-eyebrow">Runtime</p>
        <h1 className="pc-page-title">Intelligence{companyLabel ? ` — ${companyLabel}` : ""}</h1>
        <p className="pc-page-desc">
          Company-scoped runtime view of coordination health, self-improvement, and data promotion.
        </p>
      </div>

      {intelError ? (
        <p className="text-xs text-destructive">
          {intelError instanceof Error ? intelError.message : "Failed to load intelligence summary"}
        </p>
      ) : null}

      {/* ── KPI strip ─────────────────────────────────────────── */}
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

      {/* ── Tabbed sections ───────────────────────────────────── */}
      <Tabs defaultValue="self-improvement" className="w-full">
        <TabsList className="border-admin-border">
          <TabsTrigger value="self-improvement" className="text-xs">Self-improvement</TabsTrigger>
          <TabsTrigger value="promote" className="text-xs">Promote skills</TabsTrigger>
          <TabsTrigger value="memory" className="text-xs">Memory engine</TabsTrigger>
          <TabsTrigger value="signals" className="text-xs">Signals & feed</TabsTrigger>
          <TabsTrigger value="goals" className="text-xs">Goals</TabsTrigger>
        </TabsList>

        {/* ── Tab: Self-improvement ───────────────────────────── */}
        <TabsContent value="self-improvement" className="space-y-4 pt-2">
          <Card className="pc-panel border-admin-border">
            <CardHeader className="pb-2">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div>
                  <CardTitle className="text-base">Self-improvement loop</CardTitle>
                  <CardDescription>
                    Failure telemetry, replay-gated fixes, rollback safety, and weekly nudges.
                  </CardDescription>
                </div>
                <div className="flex gap-2">
                  <Button size="sm" variant="outline" className="h-7 border-admin-border text-[11px]" disabled={generateProposals.isPending} onClick={() => generateProposals.mutate()}>
                    {generateProposals.isPending ? "Generating…" : "Generate proposals"}
                  </Button>
                  <Button size="sm" variant="outline" className="h-7 border-admin-border text-[11px]" disabled={runWeeklyNudge.isPending} onClick={() => runWeeklyNudge.mutate()}>
                    {runWeeklyNudge.isPending ? "Running…" : "Run weekly nudge"}
                  </Button>
                </div>
              </div>
            </CardHeader>
            <CardContent className="grid grid-cols-2 gap-3 pt-0 sm:grid-cols-3 lg:grid-cols-6">
              {[
                { v: selfImprove?.total_failures_7d ?? 0, l: "Failures (7d)" },
                { v: `${Math.round((selfImprove?.first_pass_success_rate_7d ?? 1) * 100)}%`, l: "First-pass success" },
                { v: `${Math.round((selfImprove?.repeat_failure_rate_7d ?? 0) * 100)}%`, l: "Repeat failure rate" },
                { v: selfImprove?.proposals_created_7d ?? 0, l: "Proposals (7d)" },
                { v: selfImprove?.proposals_applied_7d ?? 0, l: "Applied (7d)" },
                { v: `${Math.round((selfImprove?.rollback_rate_7d ?? 0) * 100)}%`, l: "Rollback rate" },
              ].map((m) => (
                <div key={m.l} className="rounded border border-admin-border px-3 py-2">
                  <p className="font-mono text-lg">{m.v}</p>
                  <p className="text-[11px] text-muted-foreground">{m.l}</p>
                </div>
              ))}
            </CardContent>
          </Card>

          <Card className="pc-panel border-admin-border">
            <CardHeader className="pb-2">
              <CardTitle className="text-base">Portability and operator surface</CardTitle>
              <CardDescription>
                Discoverability layer for migration, skill portability, gateway parity, and runtime cost posture.
              </CardDescription>
            </CardHeader>
            <CardContent className="grid gap-2 text-[11px] text-muted-foreground sm:grid-cols-2">
              <p>
                AgentSkills portability endpoints are available for import/export so skills can move across systems
                without losing provenance.
              </p>
              <p>
                Legacy migration follows a dry-run-first contract to safely bring skills, memory, and allowlists from
                prior agent setups.
              </p>
              <p>
                Runtime portability matrix surfaces backend options with hibernation hints for low-idle-cost operation.
              </p>
              <p>
                Keep full Hermes import manual in Marketplace; curated bootstrap stays scoped and reversible by pack.
              </p>
              <div className="sm:col-span-2">
                <Link
                  href="/workspace/marketplace"
                  className="inline-flex items-center gap-1 text-[11px] text-primary underline-offset-2 hover:underline"
                >
                  Open marketplace actions
                </Link>
              </div>
            </CardContent>
          </Card>

          <Card className="pc-panel border-admin-border">
            <CardHeader className="pb-2">
              <CardTitle className="text-base">Proposal queue</CardTitle>
              <CardDescription>Replay and apply self-improvement proposals.</CardDescription>
            </CardHeader>
            <CardContent className="p-0">
              <ScrollArea className="h-[min(46vh,420px)]">
                <div className="min-w-[760px]">
                  <div className="pc-table-header grid-cols-[180px_100px_110px_100px_120px_120px]">
                    <span>Created</span><span>Status</span><span>Patch</span><span>Target</span><span>Replay</span><span>Apply</span>
                  </div>
                  {proposalsLoading ? (
                    Array.from({ length: 5 }).map((_, i) => (
                      <div key={i} className="flex gap-2 border-b border-admin-border px-3 py-3">
                        <Skeleton className="h-4 w-28" /><Skeleton className="h-4 w-20" /><Skeleton className="h-4 flex-1" />
                      </div>
                    ))
                  ) : proposalsError ? (
                    <p className="px-4 py-4 text-xs text-destructive">{proposalsError instanceof Error ? proposalsError.message : "Failed to load proposals"}</p>
                  ) : proposals.length === 0 ? (
                    <div className="px-4 py-6">
                      <p className="text-sm text-muted-foreground">No proposals yet.</p>
                      <p className="mt-1 text-[11px] text-muted-foreground/70">Proposals are generated from run failures. Click "Generate proposals" above, or wait for the weekly nudge to surface patterns automatically.</p>
                    </div>
                  ) : (
                    proposals.map((p) => {
                      const canReplay = p.status === "proposed" || p.status === "replay_failed";
                      const canApply = p.status === "replay_passed";
                      return (
                        <div key={p.id} className="pc-table-row grid grid-cols-[180px_100px_110px_100px_120px_120px] gap-2 border-b border-admin-border">
                          <span className="truncate font-mono text-[10px] text-muted-foreground" title={p.created_at}>{p.created_at}</span>
                          <span><Badge variant={p.status === "applied" ? "default" : p.status === "replay_failed" ? "destructive" : "outline"} className="text-[9px]">{p.status.replace(/_/g, " ")}</Badge></span>
                          <span className="truncate text-[11px] text-foreground/90" title={p.patch_kind}>{p.patch_kind}</span>
                          <span className="truncate font-mono text-[10px] text-muted-foreground" title={p.target_surface}>{p.target_surface}</span>
                          <span>
                            <Button size="sm" variant="outline" className="h-7 border-admin-border text-[10px]" disabled={!canReplay || replayProposal.isPending} onClick={() => replayProposal.mutate(p.id)}>
                              {replayProposal.isPending ? "…" : "Replay"}
                            </Button>
                          </span>
                          <span>
                            {confirmingApply === p.id ? (
                              <span className="flex gap-1">
                                <Button size="sm" variant="destructive" className="h-7 text-[10px]" disabled={applyProposal.isPending} onClick={() => { applyProposal.mutate(p.id); setConfirmingApply(null); }}>
                                  Confirm
                                </Button>
                                <Button size="sm" variant="ghost" className="h-7 text-[10px]" onClick={() => setConfirmingApply(null)}>Cancel</Button>
                              </span>
                            ) : (
                              <Button size="sm" variant="outline" className="h-7 border-admin-border text-[10px]" disabled={!canApply || applyProposal.isPending} onClick={() => setConfirmingApply(p.id)}>
                                {applyProposal.isPending ? "…" : "Apply"}
                              </Button>
                            )}
                          </span>
                        </div>
                      );
                    })
                  )}
                </div>
              </ScrollArea>
            </CardContent>
          </Card>
        </TabsContent>

        {/* ── Tab: Promote skills ─────────────────────────────── */}
        <TabsContent value="promote" className="space-y-4 pt-2">
          <Card className="pc-panel border-admin-border">
            <CardHeader className="pb-2">
              <CardTitle className="text-base">Promote skills & beliefs</CardTitle>
              <CardDescription>
                Publish reusable know-how into the operational graph so it can be searched, reviewed, and included in `llm-context`.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4 pt-0">
              <div className="rounded border border-admin-border bg-black/10 p-3">
                <p className="text-[11px] font-medium text-foreground/90">Quick start</p>
                <ol className="mt-2 space-y-1 text-[10px] text-muted-foreground">
                  <li>1. Add one skill or belief below.</li>
                  <li>2. Review the preview card before publishing.</li>
                  <li>3. Use bulk JSON only for advanced imports or migrations.</li>
                </ol>
              </div>

              <div className="grid gap-4 lg:grid-cols-2">
                <div className="space-y-3 rounded border border-admin-border p-3">
                  <div>
                    <p className="text-[11px] font-medium text-foreground/90">
                      Promote skill <span className="text-muted-foreground">(from semantic vault)</span>
                    </p>
                    <p className="mt-1 text-[10px] text-muted-foreground">
                      Best for one skill at a time. Required fields are marked.
                    </p>
                  </div>

                  <div className="grid gap-2">
                    <Input
                      className="h-8 border-admin-border bg-black/20 font-mono text-xs"
                      placeholder="Skill ID*"
                      value={skillDraft.skill_id}
                      onChange={(e) => setSkillDraft((prev) => ({ ...prev, skill_id: e.target.value }))}
                    />
                    <Input
                      className="h-8 border-admin-border bg-black/20 text-xs"
                      placeholder="Title*"
                      value={skillDraft.title}
                      onChange={(e) => setSkillDraft((prev) => ({ ...prev, title: e.target.value }))}
                    />
                    <textarea
                      className="min-h-24 w-full resize-y rounded border border-admin-border bg-black/20 p-2 text-xs text-foreground placeholder:text-muted-foreground"
                      placeholder="Principle*"
                      value={skillDraft.principle}
                      onChange={(e) => setSkillDraft((prev) => ({ ...prev, principle: e.target.value }))}
                    />
                    <div className="grid gap-2 sm:grid-cols-2">
                      <Input
                        className="h-8 border-admin-border bg-black/20 text-xs"
                        placeholder="Role (optional)"
                        value={skillDraft.role}
                        onChange={(e) => setSkillDraft((prev) => ({ ...prev, role: e.target.value }))}
                      />
                      <Input
                        className="h-8 border-admin-border bg-black/20 text-xs"
                        placeholder="Task (optional)"
                        value={skillDraft.task}
                        onChange={(e) => setSkillDraft((prev) => ({ ...prev, task: e.target.value }))}
                      />
                    </div>
                  </div>

                  <div className="rounded border border-admin-border bg-black/10 p-3">
                    <p className="text-[10px] font-medium text-foreground/90">Preview</p>
                    {skillDraftReady ? (
                      <div className="mt-2 space-y-1 text-[10px]">
                        <p className="font-medium text-foreground">{skillDraft.title}</p>
                        <p className="font-mono text-muted-foreground">{skillDraft.skill_id}</p>
                        <p className="line-clamp-3 text-muted-foreground">{skillDraft.principle}</p>
                      </div>
                    ) : (
                      <p className="mt-2 text-[10px] text-muted-foreground">
                        Add a skill ID, title, and principle to preview what will be published.
                      </p>
                    )}
                  </div>

                  <div className="flex items-center gap-2">
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 border-admin-border text-[10px]"
                      disabled={promoteRoodb.isPending || !skillDraftReady}
                      onClick={() =>
                        promoteRoodb.mutate([
                          {
                            skill_id: skillDraft.skill_id.trim(),
                            title: skillDraft.title.trim(),
                            principle: skillDraft.principle.trim(),
                            role: skillDraft.role.trim() || undefined,
                            task: skillDraft.task.trim() || undefined,
                          },
                        ])
                      }
                    >
                      {promoteRoodb.isPending ? "Publishing…" : "Publish skill"}
                    </Button>
                    {promoteRoodb.isSuccess && (
                      <p className="text-[10px] text-green-400">
                        Promoted {(promoteRoodb.data as Record<string, number>)?.promoted ?? "?"} skill(s).
                      </p>
                    )}
                  </div>
                  {promoteRoodb.isError && (
                    <p className="text-[10px] text-destructive">
                      {promoteRoodb.error instanceof Error ? promoteRoodb.error.message : "Promotion failed"}
                    </p>
                  )}

                  <details className="rounded border border-admin-border bg-black/10 p-3">
                    <summary className="cursor-pointer text-[10px] font-medium text-foreground/90">
                      Advanced: bulk JSON import
                    </summary>
                    <div className="mt-3 space-y-2">
                      <textarea
                        className="h-24 w-full resize-y rounded border border-admin-border bg-black/20 p-2 font-mono text-[10px] text-foreground/90 placeholder:text-muted-foreground"
                        placeholder={'[\n  { "skill_id": "retry-backoff", "title": "Retry with backoff", "principle": "When a tool call times out..." }\n]'}
                        value={roodbJsonInput}
                        onChange={(e) => {
                          setRoodbJsonInput(e.target.value);
                        }}
                      />
                      {parsedSkillBulk.error ? (
                        <p className="text-[10px] text-destructive">{parsedSkillBulk.error}</p>
                      ) : parsedSkillBulk.items ? (
                        <p className="text-[10px] text-muted-foreground">
                          Ready to import {parsedSkillBulk.items.length} item(s).
                        </p>
                      ) : null}
                      <Button
                        size="sm"
                        variant="outline"
                        className="h-7 border-admin-border text-[10px]"
                        disabled={promoteRoodb.isPending || !parsedSkillBulk.items?.length}
                        onClick={() => promoteRoodb.mutate(parsedSkillBulk.items ?? [])}
                      >
                        {promoteRoodb.isPending ? "Publishing…" : "Publish bulk skills"}
                      </Button>
                    </div>
                  </details>
                </div>

                <div className="space-y-3 rounded border border-admin-border p-3">
                  <div>
                    <p className="text-[11px] font-medium text-foreground/90">
                      Import belief <span className="text-muted-foreground">(from local graph)</span>
                    </p>
                    <p className="mt-1 text-[10px] text-muted-foreground">
                      Best for one insight, policy, or learned rule at a time.
                    </p>
                  </div>

                  <div className="grid gap-2">
                    <Input
                      className="h-8 border-admin-border bg-black/20 text-xs"
                      placeholder="Title (optional)"
                      value={beliefDraft.title}
                      onChange={(e) => setBeliefDraft((prev) => ({ ...prev, title: e.target.value }))}
                    />
                    <textarea
                      className="min-h-24 w-full resize-y rounded border border-admin-border bg-black/20 p-2 text-xs text-foreground placeholder:text-muted-foreground"
                      placeholder="Belief or learned rule*"
                      value={beliefDraft.content}
                      onChange={(e) => setBeliefDraft((prev) => ({ ...prev, content: e.target.value }))}
                    />
                    <div className="grid gap-2 sm:grid-cols-2">
                      <Input
                        className="h-8 border-admin-border bg-black/20 text-xs"
                        placeholder="Confidence"
                        value={beliefDraft.confidence}
                        onChange={(e) => setBeliefDraft((prev) => ({ ...prev, confidence: e.target.value }))}
                      />
                      <Input
                        className="h-8 border-admin-border bg-black/20 text-xs"
                        placeholder="Tags (comma-separated)"
                        value={beliefDraft.tags}
                        onChange={(e) => setBeliefDraft((prev) => ({ ...prev, tags: e.target.value }))}
                      />
                    </div>
                  </div>

                  <div className="rounded border border-admin-border bg-black/10 p-3">
                    <p className="text-[10px] font-medium text-foreground/90">Preview</p>
                    {beliefDraftReady ? (
                      <div className="mt-2 space-y-1 text-[10px]">
                        <p className="font-medium text-foreground">
                          {beliefDraft.title.trim() || "Untitled belief"}
                        </p>
                        <p className="line-clamp-3 text-muted-foreground">{beliefDraft.content}</p>
                        <p className="font-mono text-muted-foreground">
                          tags: {splitTagList(beliefDraft.tags).join(", ") || "none"}
                        </p>
                      </div>
                    ) : (
                      <p className="mt-2 text-[10px] text-muted-foreground">
                        Add belief content to preview what will be published.
                      </p>
                    )}
                  </div>

                  <div className="flex items-center gap-2">
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 border-admin-border text-[10px]"
                      disabled={importLadybug.isPending || !beliefDraftReady}
                      onClick={() =>
                        importLadybug.mutate([
                          {
                            title: beliefDraft.title.trim() || undefined,
                            content: beliefDraft.content.trim(),
                            confidence: Number.parseFloat(beliefDraft.confidence) || 0.8,
                            tags: splitTagList(beliefDraft.tags),
                          },
                        ])
                      }
                    >
                      {importLadybug.isPending ? "Publishing…" : "Publish belief"}
                    </Button>
                    {importLadybug.isSuccess && (
                      <p className="text-[10px] text-green-400">
                        Imported {(importLadybug.data as Record<string, number>)?.imported ?? "?"} belief(s).
                      </p>
                    )}
                  </div>
                  {importLadybug.isError && (
                    <p className="text-[10px] text-destructive">
                      {importLadybug.error instanceof Error ? importLadybug.error.message : "Import failed"}
                    </p>
                  )}

                  <details className="rounded border border-admin-border bg-black/10 p-3">
                    <summary className="cursor-pointer text-[10px] font-medium text-foreground/90">
                      Advanced: bulk JSON import
                    </summary>
                    <div className="mt-3 space-y-2">
                      <textarea
                        className="h-24 w-full resize-y rounded border border-admin-border bg-black/20 p-2 font-mono text-[10px] text-foreground/90 placeholder:text-muted-foreground"
                        placeholder={'[\n  { "content": "When formatting fails, validate XML before retry.", "confidence": 0.85, "tags": ["skill"] }\n]'}
                        value={ladybugJsonInput}
                        onChange={(e) => {
                          setLadybugJsonInput(e.target.value);
                        }}
                      />
                      {parsedBeliefBulk.error ? (
                        <p className="text-[10px] text-destructive">{parsedBeliefBulk.error}</p>
                      ) : parsedBeliefBulk.items ? (
                        <p className="text-[10px] text-muted-foreground">
                          Ready to import {parsedBeliefBulk.items.length} item(s).
                        </p>
                      ) : null}
                      <Button
                        size="sm"
                        variant="outline"
                        className="h-7 border-admin-border text-[10px]"
                        disabled={importLadybug.isPending || !parsedBeliefBulk.items?.length}
                        onClick={() => importLadybug.mutate(parsedBeliefBulk.items ?? [])}
                      >
                        {importLadybug.isPending ? "Publishing…" : "Publish bulk beliefs"}
                      </Button>
                    </div>
                  </details>
                </div>
              </div>

              <div>
                <p className="mb-1 text-[11px] font-medium text-foreground/90">Promotion history</p>
                <ScrollArea className="h-[min(28vh,240px)]">
                  <div className="min-w-[600px]">
                    <div className="pc-table-header grid-cols-[100px_80px_1fr_80px_100px_80px]">
                      <span>When</span><span>Source</span><span>Source ID</span><span>Status</span><span>By</span><span />
                    </div>
                    {promotionsLoading ? (
                      Array.from({ length: 3 }).map((_, i) => (
                        <div key={i} className="flex gap-2 border-b border-admin-border px-3 py-3"><Skeleton className="h-4 w-24" /><Skeleton className="h-4 flex-1" /></div>
                      ))
                    ) : promotions.length === 0 ? (
                      <div className="px-4 py-4">
                        <p className="text-[11px] text-muted-foreground">No published skills or beliefs yet.</p>
                        <p className="mt-1 text-[10px] text-muted-foreground/70">
                          Publish one item above to make it available to operators, approvals, and context assembly.
                        </p>
                      </div>
                    ) : (
                      promotions.map((p) => (
                        <div key={p.id} className="pc-table-row grid grid-cols-[100px_80px_1fr_80px_100px_80px] gap-2 border-b border-admin-border">
                          <span className="truncate font-mono text-[10px] text-muted-foreground" title={p.created_at}>{p.created_at.slice(0, 10)}</span>
                          <span><Badge variant={p.source_store === "roodb" ? "secondary" : "outline"} className="text-[9px]">{p.source_store === "roodb" ? "vault" : "local"}</Badge></span>
                          <span className="truncate font-mono text-[10px] text-muted-foreground" title={p.source_id}>{p.source_id}</span>
                          <span><Badge variant={p.status === "promoted" ? "default" : "destructive"} className="text-[9px]">{p.status}</Badge></span>
                          <span className="truncate text-[10px] text-muted-foreground">{p.promoted_by}</span>
                          <span>
                            {p.status === "promoted" && (
                              confirmingRollback === p.id ? (
                                <span className="flex gap-1">
                                  <Button size="sm" variant="destructive" className="h-6 text-[9px]" disabled={rollbackPromotion.isPending} onClick={() => { rollbackPromotion.mutate(p.id); setConfirmingRollback(null); }}>
                                    Confirm
                                  </Button>
                                  <Button size="sm" variant="ghost" className="h-6 text-[9px]" onClick={() => setConfirmingRollback(null)}>No</Button>
                                </span>
                              ) : (
                                <Button size="sm" variant="outline" className="h-6 border-admin-border text-[9px]" disabled={rollbackPromotion.isPending} onClick={() => setConfirmingRollback(p.id)}>
                                  Rollback
                                </Button>
                              )
                            )}
                          </span>
                        </div>
                      ))
                    )}
                  </div>
                </ScrollArea>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        {/* ── Tab: Memory engine ──────────────────────────────── */}
        <TabsContent value="memory" className="space-y-4 pt-2">
          <div className="grid gap-4 xl:grid-cols-[1.2fr_0.8fr]">
            <Card className="pc-panel border-admin-border">
              <CardHeader className="pb-2">
                <CardTitle className="text-base">Multimodal ingest</CardTitle>
                <CardDescription>
                  Queue web, file, image, and audio sources into canonical memory nodes backed by artifacts and chunks.
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-4 pt-0">
                <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                  {(memoryMetrics.data?.artifact_status_counts ?? []).slice(0, 4).map((row) => (
                    <div key={row.status} className="rounded border border-admin-border px-3 py-2">
                      <p className="font-mono text-lg">{row.count}</p>
                      <p className="text-[11px] text-muted-foreground">{row.status.replace(/_/g, " ")}</p>
                    </div>
                  ))}
                </div>
                <div className="grid gap-3 lg:grid-cols-2">
                  <div className="space-y-2 rounded border border-admin-border p-3">
                    <p className="text-[11px] font-medium text-foreground/90">Ingest web page</p>
                    <Input
                      className="h-8 border-admin-border bg-black/20 text-xs"
                      placeholder="https://example.com/notes"
                      value={webIngestUrl}
                      onChange={(e) => setWebIngestUrl(e.target.value)}
                    />
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 border-admin-border text-[10px]"
                      disabled={ingestMemory.isPending || !webIngestUrl.trim()}
                      onClick={() =>
                        ingestMemory.mutate({
                          endpoint: "web",
                          body: { url: webIngestUrl.trim(), scope: "shared" },
                        })
                      }
                    >
                      {ingestMemory.isPending ? "Queueing…" : "Queue web ingest"}
                    </Button>
                  </div>
                  <div className="space-y-2 rounded border border-admin-border p-3">
                    <p className="text-[11px] font-medium text-foreground/90">Ingest file</p>
                    <Input
                      className="h-8 border-admin-border bg-black/20 font-mono text-xs"
                      placeholder="docs/strategy.md or /abs/path/report.pdf"
                      value={fileIngestPath}
                      onChange={(e) => setFileIngestPath(e.target.value)}
                    />
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 border-admin-border text-[10px]"
                      disabled={ingestMemory.isPending || !fileIngestPath.trim()}
                      onClick={() =>
                        ingestMemory.mutate({
                          endpoint: "file",
                          body: { path: fileIngestPath.trim(), scope: "shared" },
                        })
                      }
                    >
                      {ingestMemory.isPending ? "Queueing…" : "Queue file ingest"}
                    </Button>
                  </div>
                  <div className="space-y-2 rounded border border-admin-border p-3">
                    <p className="text-[11px] font-medium text-foreground/90">Ingest image OCR/caption</p>
                    <textarea
                      className="min-h-24 w-full resize-y rounded border border-admin-border bg-black/20 p-2 text-xs text-foreground placeholder:text-muted-foreground"
                      placeholder="Paste OCR or caption text for an image artifact"
                      value={imageIngestText}
                      onChange={(e) => setImageIngestText(e.target.value)}
                    />
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 border-admin-border text-[10px]"
                      disabled={ingestMemory.isPending || !imageIngestText.trim()}
                      onClick={() =>
                        ingestMemory.mutate({
                          endpoint: "image",
                          body: { extracted_text: imageIngestText.trim(), scope: "shared" },
                        })
                      }
                    >
                      {ingestMemory.isPending ? "Queueing…" : "Queue image ingest"}
                    </Button>
                  </div>
                  <div className="space-y-2 rounded border border-admin-border p-3">
                    <p className="text-[11px] font-medium text-foreground/90">Ingest audio transcript</p>
                    <textarea
                      className="min-h-24 w-full resize-y rounded border border-admin-border bg-black/20 p-2 text-xs text-foreground placeholder:text-muted-foreground"
                      placeholder="Paste transcript text for an audio artifact"
                      value={audioIngestText}
                      onChange={(e) => setAudioIngestText(e.target.value)}
                    />
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 border-admin-border text-[10px]"
                      disabled={ingestMemory.isPending || !audioIngestText.trim()}
                      onClick={() =>
                        ingestMemory.mutate({
                          endpoint: "audio",
                          body: { extracted_text: audioIngestText.trim(), scope: "shared" },
                        })
                      }
                    >
                      {ingestMemory.isPending ? "Queueing…" : "Queue audio ingest"}
                    </Button>
                  </div>
                </div>
                {ingestMemory.isError ? (
                  <p className="text-[11px] text-destructive">
                    {ingestMemory.error instanceof Error ? ingestMemory.error.message : "Ingest failed"}
                  </p>
                ) : null}
              </CardContent>
            </Card>

            <Card className="pc-panel border-admin-border">
              <CardHeader className="pb-2">
                <CardTitle className="text-base">Artifact queue</CardTitle>
                <CardDescription>
                  Extraction status, retries, and the handoff between source artifacts and canonical memory rows.
                </CardDescription>
              </CardHeader>
              <CardContent className="p-0">
                <ScrollArea className="h-[min(48vh,420px)]">
                  <div className="min-w-[420px]">
                    <div className="pc-table-header grid-cols-[84px_78px_1fr_80px_90px]">
                      <span>When</span><span>Type</span><span>Source</span><span>Status</span><span>Action</span>
                    </div>
                    {artifactsLoading ? (
                      Array.from({ length: 4 }).map((_, i) => (
                        <div key={i} className="flex gap-2 border-b border-admin-border px-3 py-3">
                          <Skeleton className="h-4 w-20" />
                          <Skeleton className="h-4 flex-1" />
                        </div>
                      ))
                    ) : artifacts.length === 0 ? (
                      <p className="px-4 py-6 text-sm text-muted-foreground">
                        No artifacts yet. Queue a web page, file, image, or transcript above.
                      </p>
                    ) : (
                      artifacts.map((artifact: HsmMemoryArtifact) => (
                        <div
                          key={artifact.id}
                          className="pc-table-row grid grid-cols-[84px_78px_1fr_80px_90px] gap-2 border-b border-admin-border"
                        >
                          <span className="truncate font-mono text-[10px] text-muted-foreground">
                            {artifact.created_at.slice(0, 10)}
                          </span>
                          <span>
                            <Badge variant="outline" className="text-[9px]">
                              {artifact.media_type}
                            </Badge>
                          </span>
                          <span className="min-w-0">
                            <p className="truncate text-[11px] text-foreground/90">
                              {artifact.title || artifact.source_uri || artifact.id}
                            </p>
                            {artifact.last_error ? (
                              <p className="truncate text-[9px] text-destructive/90">{artifact.last_error}</p>
                            ) : null}
                          </span>
                          <span>
                            <Badge
                              variant={
                                artifact.extraction_status === "indexed"
                                  ? "default"
                                  : artifact.extraction_status === "dead_letter"
                                    ? "destructive"
                                    : "outline"
                              }
                              className="text-[9px]"
                            >
                              {artifact.extraction_status.replace(/_/g, " ")}
                            </Badge>
                          </span>
                          <span className="flex gap-1">
                            {artifact.memory_id ? (
                              <Button
                                size="sm"
                                variant="ghost"
                                className="h-6 px-2 text-[9px]"
                                onClick={() => setSelectedMemoryId(artifact.memory_id ?? null)}
                              >
                                Inspect
                              </Button>
                            ) : null}
                            {artifact.extraction_status === "retry_waiting" ||
                            artifact.extraction_status === "dead_letter" ? (
                              <Button
                                size="sm"
                                variant="outline"
                                className="h-6 border-admin-border px-2 text-[9px]"
                                disabled={retryArtifact.isPending}
                                onClick={() => retryArtifact.mutate(artifact.id)}
                              >
                                Retry
                              </Button>
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

          <div className="grid gap-4 xl:grid-cols-[1fr_1fr]">
            <Card className="pc-panel border-admin-border">
              <CardHeader className="pb-2">
                <CardTitle className="text-base">Retrieval debugger</CardTitle>
                <CardDescription>
                  Inspect why a memory matched: vector, graph, temporal, or FTS channels.
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-3 pt-0">
                <div className="grid gap-2 md:grid-cols-[1fr_auto]">
                  <Input
                    className="h-8 border-admin-border bg-black/20 text-xs"
                    placeholder="Query the memory graph…"
                    value={retrievalDraft}
                    onChange={(e) => setRetrievalDraft(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") setRetrievalApplied(retrievalDraft.trim());
                    }}
                  />
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-8 border-admin-border text-[10px]"
                    onClick={() => setRetrievalApplied(retrievalDraft.trim())}
                  >
                    Run search
                  </Button>
                </div>
                <div className="grid gap-2 sm:grid-cols-3">
                  <Input
                    className="h-8 border-admin-border bg-black/20 text-xs"
                    placeholder="Entity type (optional)"
                    value={memoryEntityType}
                    onChange={(e) => setMemoryEntityType(e.target.value)}
                  />
                  <Input
                    className="h-8 border-admin-border bg-black/20 text-xs"
                    placeholder="Entity ID (optional)"
                    value={memoryEntityId}
                    onChange={(e) => setMemoryEntityId(e.target.value)}
                  />
                  <label className="flex items-center gap-2 rounded border border-admin-border px-3 py-2 text-[11px] text-muted-foreground">
                    <input
                      type="checkbox"
                      checked={memoryLatestOnly}
                      onChange={(e) => setMemoryLatestOnly(e.target.checked)}
                    />
                    latest only
                  </label>
                </div>
                <ScrollArea className="h-[min(42vh,360px)]">
                  <div className="space-y-2 pr-2">
                    {retrievalDebug.isLoading ? (
                      Array.from({ length: 4 }).map((_, i) => <Skeleton key={i} className="h-20 rounded-md" />)
                    ) : retrievalDebug.error ? (
                      <p className="text-[11px] text-destructive">
                        {retrievalDebug.error instanceof Error ? retrievalDebug.error.message : "Retrieval failed"}
                      </p>
                    ) : !(retrievalDebug.data?.matches?.length) ? (
                      <p className="text-sm text-muted-foreground">
                        Run a query to see chunk-level matches and their retrieval channels.
                      </p>
                    ) : (
                      retrievalDebug.data.matches.map((match: HsmMemoryMatch) => (
                        <div key={match.id} className="rounded border border-admin-border p-3">
                          <div className="flex items-start justify-between gap-2">
                            <div className="min-w-0">
                              <p className="truncate font-mono text-[10px] text-muted-foreground">{match.id}</p>
                              <p className="mt-1 text-[11px] text-foreground/90">
                                matched via {match.matched_via.join(", ") || "unknown"}
                              </p>
                              {match.lineage_summary ? (
                                <p className="text-[10px] text-muted-foreground">{match.lineage_summary}</p>
                              ) : null}
                            </div>
                            <Button
                              size="sm"
                              variant="ghost"
                              className="h-6 px-2 text-[9px]"
                              onClick={() => setSelectedMemoryId(match.id)}
                            >
                              Inspect
                            </Button>
                          </div>
                          <div className="mt-2 space-y-1">
                            {match.supporting_chunks.map((chunk) => (
                              <div key={chunk.chunk_id} className="rounded border border-admin-border/70 bg-black/10 px-2 py-1.5">
                                <p className="font-mono text-[9px] text-muted-foreground">
                                  {chunk.modality} #{chunk.chunk_index}
                                  {chunk.source_label ? ` · ${chunk.source_label}` : ""}
                                </p>
                                <p className="mt-1 text-[10px] text-muted-foreground">{chunk.text}</p>
                              </div>
                            ))}
                          </div>
                        </div>
                      ))
                    )}
                  </div>
                </ScrollArea>
              </CardContent>
            </Card>

            <Card className="pc-panel border-admin-border">
              <CardHeader className="pb-2">
                <CardTitle className="text-base">Memory inspector</CardTitle>
                <CardDescription>
                  Canonical node, artifact provenance, chunk support, and version lineage.
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-3 pt-0">
                {!selectedMemoryId ? (
                  <p className="text-sm text-muted-foreground">
                    Select a memory from the artifact queue or retrieval debugger.
                  </p>
                ) : !memoryInspect ? (
                  <Skeleton className="h-40 rounded-md" />
                ) : (
                  <>
                    <div className="rounded border border-admin-border p-3">
                      <div className="flex flex-wrap items-center gap-2">
                        <p className="text-sm font-medium text-foreground">{memoryInspect.memory.title}</p>
                        <Badge variant={memoryInspect.memory.is_latest ? "default" : "outline"} className="text-[9px]">
                          v{memoryInspect.memory.version}
                        </Badge>
                        {memoryInspect.memory.entity_type ? (
                          <Badge variant="secondary" className="text-[9px]">
                            {memoryInspect.memory.entity_type}
                          </Badge>
                        ) : null}
                      </div>
                      <p className="mt-2 text-[11px] text-muted-foreground">
                        {memoryInspect.memory.summary_l1 || memoryInspect.memory.body}
                      </p>
                      <div className="mt-2 flex flex-wrap gap-3 font-mono text-[9px] text-muted-foreground">
                        <span>artifacts {memoryInspect.memory.source_artifact_count}</span>
                        <span>chunks {memoryInspect.memory.chunk_count}</span>
                        {memoryInspect.memory.event_date ? <span>event {memoryInspect.memory.event_date}</span> : null}
                        {memoryInspect.memory.document_date ? (
                          <span>doc {memoryInspect.memory.document_date}</span>
                        ) : null}
                      </div>
                    </div>
                    <div className="grid gap-3 lg:grid-cols-2">
                      <div className="rounded border border-admin-border p-3">
                        <p className="text-[11px] font-medium text-foreground/90">Artifacts</p>
                        <div className="mt-2 space-y-2">
                          {memoryInspect.artifacts.map((artifact) => (
                            <div key={artifact.id} className="rounded border border-admin-border/70 bg-black/10 px-2 py-2">
                              <p className="text-[10px] text-foreground/90">
                                {artifact.title || artifact.source_uri || artifact.id}
                              </p>
                              <p className="font-mono text-[9px] text-muted-foreground">
                                {artifact.media_type} · {artifact.extraction_status}
                              </p>
                            </div>
                          ))}
                        </div>
                      </div>
                      <div className="rounded border border-admin-border p-3">
                        <p className="text-[11px] font-medium text-foreground/90">Lineage</p>
                        <div className="mt-2 space-y-2">
                          {memoryInspect.lineage.length === 0 ? (
                            <p className="text-[10px] text-muted-foreground">No version chain.</p>
                          ) : (
                            memoryInspect.lineage.map((row) => (
                              <div key={row.id} className="rounded border border-admin-border/70 bg-black/10 px-2 py-2">
                                <p className="font-mono text-[9px] text-muted-foreground">{row.id}</p>
                                <p className="text-[10px] text-foreground/90">
                                  v{row.version} {row.is_latest ? "latest" : "historical"}
                                </p>
                              </div>
                            ))
                          )}
                        </div>
                      </div>
                    </div>
                    <div className="rounded border border-admin-border p-3">
                      <p className="text-[11px] font-medium text-foreground/90">Supporting chunks</p>
                      <ScrollArea className="mt-2 h-[min(24vh,220px)]">
                        <div className="space-y-2 pr-2">
                          {memoryInspect.chunks.map((chunk) => (
                            <div key={chunk.id} className="rounded border border-admin-border/70 bg-black/10 px-2 py-2">
                              <p className="font-mono text-[9px] text-muted-foreground">
                                #{chunk.chunk_index} · {chunk.modality} · {chunk.token_count} tokens
                              </p>
                              <p className="mt-1 text-[10px] text-muted-foreground">
                                {chunk.redacted_text || chunk.summary_l1 || chunk.text}
                              </p>
                            </div>
                          ))}
                        </div>
                      </ScrollArea>
                    </div>
                  </>
                )}
              </CardContent>
            </Card>
          </div>
        </TabsContent>

        {/* ── Tab: Signals & feed ─────────────────────────────── */}
        <TabsContent value="signals" className="space-y-4 pt-2">
          <SignalFeed apiBase={apiBase} companyId={companyId} />

          <Card className="pc-panel border-admin-border">
            <CardHeader className="pb-2">
              <CardTitle className="text-base">Workflow feed</CardTitle>
              <CardDescription>Task lifecycle events: create, checkout, escalation, spawn, policy decisions, terminal runs.</CardDescription>
            </CardHeader>
            <CardContent className="p-0">
              <ScrollArea className="h-[min(48vh,420px)]">
                <div className="min-w-[360px] px-1">
                  <div className="pc-table-header grid-cols-[120px_1fr_100px]">
                    <span>When</span><span>Event</span><span>Subject</span>
                  </div>
                  {intelLoading ? (
                    Array.from({ length: 6 }).map((_, i) => (
                      <div key={i} className="flex gap-2 border-b border-admin-border px-3 py-3"><Skeleton className="h-4 w-24" /><Skeleton className="h-4 flex-1" /></div>
                    ))
                  ) : !intel?.workflow_feed?.length ? (
                    <p className="px-4 py-6 text-sm text-muted-foreground">No workflow events yet for this company.</p>
                  ) : (
                    intel.workflow_feed.map((e: HsmWorkflowFeedEvent) => (
                      <div key={e.id} className="pc-table-row grid grid-cols-[120px_1fr_100px] gap-2 border-b border-admin-border">
                        <span className="truncate font-mono text-[10px] text-muted-foreground">{e.created_at}</span>
                        <span className="min-w-0">
                          <Badge variant="outline" className="mr-1 text-[10px]">{actionLabel(e.action)}</Badge>
                          <span className="font-mono text-[10px] text-muted-foreground">{e.actor}</span>
                          <p className="mt-0.5 truncate font-mono text-[9px] text-muted-foreground/80">{payloadPreview(e.payload ?? {})}</p>
                        </span>
                        <span className="truncate font-mono text-[10px] text-muted-foreground" title={e.subject_id}>
                          {e.subject_type}:{e.subject_id.length > 10 ? `${e.subject_id.slice(0, 8)}…` : e.subject_id}
                        </span>
                      </div>
                    ))
                  )}
                </div>
              </ScrollArea>
            </CardContent>
          </Card>
        </TabsContent>

        {/* ── Tab: Goals ──────────────────────────────────────── */}
        <TabsContent value="goals" className="pt-2">
          <Card className="pc-panel border-admin-border">
            <CardHeader className="pb-2">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div>
                  <CardTitle className="text-base">Goals</CardTitle>
                  <CardDescription>Operational graph — same company as tasks and spend</CardDescription>
                </div>
                <Button asChild size="sm" variant="ghost" className="h-7 text-[11px] text-[#79b8ff]">
                  <Link href="/workspace/issues">Open tasks →</Link>
                </Button>
              </div>
            </CardHeader>
            <CardContent className="space-y-3 p-0">
              <div className="flex gap-2 px-4 pb-2 pt-1">
                <Input className="h-8 border-admin-border bg-black/20 font-mono text-xs" placeholder="New goal title…" value={newGoalTitle} onChange={(e) => setNewGoalTitle(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && newGoalTitle.trim()) createGoal.mutate(newGoalTitle.trim()); }} />
                <Button size="sm" variant="outline" className="shrink-0 border-admin-border text-xs" disabled={!newGoalTitle.trim() || createGoal.isPending} onClick={() => createGoal.mutate(newGoalTitle.trim())}>
                  {createGoal.isPending ? "…" : "Add"}
                </Button>
              </div>
              {goalsError ? <p className="px-4 pb-3 text-xs text-destructive">{goalsError instanceof Error ? goalsError.message : "Failed to load goals"}</p> : null}
              <ScrollArea className="h-[min(48vh,420px)]">
                <div className="min-w-[400px]">
                  <div className="pc-table-header grid-cols-[1fr_100px_80px]">
                    <span>Title</span><span>Status</span><span>Action</span>
                  </div>
                  {goalsLoading ? (
                    Array.from({ length: 5 }).map((_, i) => (
                      <div key={i} className="flex gap-2 border-b border-admin-border px-4 py-3"><Skeleton className="h-4 flex-1" /><Skeleton className="h-4 w-20" /></div>
                    ))
                  ) : goals.length === 0 ? (
                    <p className="px-4 py-6 text-sm text-muted-foreground">No goals yet. Create one above.</p>
                  ) : (
                    goals.map((g: HsmGoalRow) => (
                      <div key={g.id} className="pc-table-row grid grid-cols-[1fr_100px_80px] gap-2">
                        <span className="truncate text-xs text-foreground">{g.title}</span>
                        <span><Badge variant={statusBadgeVariant(g.status)} className="text-[10px]">{g.status}</Badge></span>
                        <span>{!goalIsTerminal(g.status) ? <CompleteGoalButton apiBase={apiBase} companyId={companyId} goalId={g.id} onDone={invalidateIntel} /> : null}</span>
                      </div>
                    ))
                  )}
                </div>
              </ScrollArea>
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
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
