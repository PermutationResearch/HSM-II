"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { CheckCircle2, ChevronDown, Map, Paperclip, Plus, Repeat, Sparkles, User } from "lucide-react";
import { Button } from "@/app/components/ui/button";
import { Checkbox } from "@/app/components/ui/checkbox";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/app/components/ui/collapsible";
import { Input } from "@/app/components/ui/input";
import { Label } from "@/app/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/app/components/ui/select";
import { Textarea } from "@/app/components/ui/textarea";
import { companyOsUrl } from "@/app/lib/company-api-url";
import { ISSUE_LABEL_SEED_DEFAULTS } from "@/app/lib/issue-label-defaults";
import { useCompanyAgents, useCompanyIssueLabels, useCompanyProjects } from "@/app/lib/hsm-queries";
import { specificationWithWorkspacePaths, truncatePath } from "@/app/lib/workspace-issue";
import { cn } from "@/app/lib/utils";

type IssueKind = "plan" | "todo";
type Recurring = "none" | "daily" | "weekly" | "monthly";
/** API: numeric `priority` on task (higher sorts first); `reviewer` adds `mode: priority:reviewer`. */
type PriorityTier = "low" | "medium" | "high" | "reviewer";

export type WorkspaceNewIssueFormProps = {
  apiBase: string;
  companyId: string;
  /**
   * Pre-seeded paths from context (e.g. open file in workspace). User can add more when `showPathAttach` is true.
   * Omit or pass [] when there is no seed.
   */
  workspacePaths?: string[];
  assigneeDisplayName: string;
  assigneePersona: string;
  idPrefix: string;
  /** Called after successful create (dialog should close). */
  onCreated?: () => void;
  /** Show workspace attachment summary when there are paths. */
  showAttachBanner?: boolean;
  /** When set (e.g. dialog), show Cancel and call this without clearing the saved draft. */
  onCloseRequest?: () => void;
  /** Show “attach from workspace” input + chips. Default true (workspace dialog and Issues page). */
  showPathAttach?: boolean;
};

function draftKey(companyId: string) {
  return `hsm-ws-new-issue-draft:${companyId}`;
}

export function WorkspaceNewIssueForm({
  apiBase,
  companyId,
  workspacePaths: seedWorkspacePathsProp = [],
  assigneeDisplayName,
  assigneePersona,
  idPrefix,
  onCreated,
  showAttachBanner = true,
  onCloseRequest,
  showPathAttach = true,
}: WorkspaceNewIssueFormProps) {
  const qc = useQueryClient();
  const { data: agentsRaw = [] } = useCompanyAgents(apiBase, companyId);
  const { data: projectsRaw = [] } = useCompanyProjects(apiBase, companyId);
  const { data: issueLabels = [] } = useCompanyIssueLabels(apiBase, companyId);

  const projectsSorted = useMemo(
    () =>
      [...projectsRaw]
        .filter((p) => (p.status ?? "active").toLowerCase() !== "archived")
        .sort((a, b) => {
          const o = (a.sort_order ?? 0) - (b.sort_order ?? 0);
          return o !== 0 ? o : a.title.localeCompare(b.title);
        }),
    [projectsRaw],
  );

  const agents = useMemo(
    () =>
      agentsRaw
        .filter((a) => (a.status ?? "").toLowerCase() !== "terminated")
        .sort((a, b) => a.name.localeCompare(b.name)),
    [agentsRaw],
  );

  const defaultPersona = assigneePersona.trim();

  const seedKey = seedWorkspacePathsProp.join("\0");
  const seedPaths = useMemo(
    () => seedWorkspacePathsProp.map((p) => p.trim()).filter(Boolean),
    [seedKey],
  );

  const [userPaths, setUserPaths] = useState<string[]>([]);
  const [removedSeed, setRemovedSeed] = useState<Set<string>>(() => new Set());
  const [pathDraft, setPathDraft] = useState("");

  useEffect(() => {
    setRemovedSeed(new Set());
  }, [seedKey]);

  const effectiveSeedPaths = useMemo(
    () => seedPaths.filter((p) => !removedSeed.has(p)),
    [seedPaths, removedSeed],
  );

  const allWorkspacePaths = useMemo(() => {
    const u = userPaths.map((p) => p.trim()).filter(Boolean);
    return [...new Set([...effectiveSeedPaths, ...u])];
  }, [effectiveSeedPaths, userPaths]);

  const [title, setTitle] = useState("");
  const [bodyExtra, setBodyExtra] = useState("");
  const [assignToAgent, setAssignToAgent] = useState(() => Boolean(defaultPersona));
  const [issueKind, setIssueKind] = useState<IssueKind>("todo");
  const [reviewerPersona, setReviewerPersona] = useState(defaultPersona);
  const [approverPersona, setApproverPersona] = useState(defaultPersona);
  const [recurring, setRecurring] = useState<Recurring>("none");
  /** Start closed so simple flows (e.g. exec) are not buried; avoids stray toggles with assignee control. */
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [recurringOpen, setRecurringOpen] = useState(false);
  const [projectId, setProjectId] = useState("");
  const [priorityTier, setPriorityTier] = useState<PriorityTier>("medium");
  const [selectedLabels, setSelectedLabels] = useState<string[]>([]);
  const [showAddLabel, setShowAddLabel] = useState(false);
  const [newLabelSlug, setNewLabelSlug] = useState("");
  const [newLabelName, setNewLabelName] = useState("");
  /** True when seed/suggested labels were skipped (API or DB not deployed). */
  const [labelsCatalogSkipped, setLabelsCatalogSkipped] = useState(false);
  /** Skip label tags; adds handoff note + mode ref for reviewer triage. */
  const [labelsDeferredToReviewer, setLabelsDeferredToReviewer] = useState(false);

  const pathKey = allWorkspacePaths.join("\0");
  const pathSeenRef = useRef<string | null>(null);

  useEffect(() => {
    setAssignToAgent(Boolean(defaultPersona));
    setReviewerPersona(defaultPersona);
    setApproverPersona(defaultPersona);
  }, [defaultPersona]);

  useEffect(() => {
    setLabelsCatalogSkipped(false);
    setLabelsDeferredToReviewer(false);
  }, [companyId]);

  useEffect(() => {
    if (!companyId) return;
    try {
      const raw = localStorage.getItem(draftKey(companyId));
      if (raw) {
        const j = JSON.parse(raw) as { title?: string; bodyExtra?: string };
        setTitle(typeof j.title === "string" ? j.title : "");
        setBodyExtra(typeof j.bodyExtra === "string" ? j.bodyExtra : "");
      } else {
        setTitle("");
        setBodyExtra("");
      }
    } catch {
      setTitle("");
      setBodyExtra("");
    }
  }, [companyId]);

  useEffect(() => {
    if (!companyId) return;
    const t = window.setTimeout(() => {
      try {
        localStorage.setItem(draftKey(companyId), JSON.stringify({ title, bodyExtra }));
      } catch {
        /* ignore */
      }
    }, 400);
    return () => window.clearTimeout(t);
  }, [companyId, title, bodyExtra]);

  useEffect(() => {
    if (pathSeenRef.current === null) {
      pathSeenRef.current = pathKey;
      return;
    }
    if (pathSeenRef.current !== pathKey) {
      pathSeenRef.current = pathKey;
      setTitle("");
      setBodyExtra("");
    }
  }, [pathKey]);

  function attachWorkspacePath() {
    const p = pathDraft.trim();
    if (!p) return;
    setUserPaths((prev) => (prev.includes(p) ? prev : [...prev, p]));
    setPathDraft("");
  }

  function removeAttachedPath(p: string) {
    if (userPaths.includes(p)) {
      setUserPaths((prev) => prev.filter((x) => x !== p));
    } else if (seedPaths.includes(p)) {
      setRemovedSeed((prev) => new Set(prev).add(p));
    }
  }

  const buildCapabilityRefs = useCallback((): Record<string, unknown>[] => {
    const caps: Record<string, unknown>[] = [];
    caps.push(
      issueKind === "plan"
        ? { kind: "mode", ref: "plan" }
        : { kind: "mode", ref: "todo" },
    );
    if (priorityTier === "reviewer") {
      caps.push({ kind: "mode", ref: "priority:reviewer" });
    }
    if (reviewerPersona.trim()) {
      caps.push({ kind: "agent", ref: reviewerPersona.trim(), role: "reviewer" });
    }
    if (approverPersona.trim()) {
      caps.push({ kind: "agent", ref: approverPersona.trim(), role: "approver" });
    }
    if (recurring !== "none") {
      caps.push({ kind: "mode", ref: `recurring:${recurring}` });
    }
    if (labelsDeferredToReviewer) {
      caps.push({ kind: "mode", ref: "labels:reviewer" });
    } else {
      const slugs = [...selectedLabels].map((s) => s.trim()).filter(Boolean).sort();
      for (const slug of slugs) {
        caps.push({ kind: "label", ref: slug });
      }
    }
    return caps;
  }, [
    issueKind,
    priorityTier,
    reviewerPersona,
    approverPersona,
    recurring,
    selectedLabels,
    labelsDeferredToReviewer,
  ]);

  function toggleLabelSlug(slug: string) {
    setLabelsDeferredToReviewer(false);
    setSelectedLabels((prev) => (prev.includes(slug) ? prev.filter((s) => s !== slug) : [...prev, slug]));
  }

  const seedDefaultLabels = useMutation({
    mutationFn: async (): Promise<{ skipped?: boolean } | void> => {
      const seedUrl = companyOsUrl(
        apiBase,
        `/api/company/companies/${companyId}/issue-labels/seed-defaults`,
      );
      const r = await fetch(seedUrl, { method: "POST" });
      if (r.ok) return;

      const j = (await r.json().catch(() => ({}))) as { error?: string };

      // Older hsm_console without POST …/seed-defaults — create each label (same as server seed).
      if (r.status === 404) {
        const createUrl = companyOsUrl(apiBase, `/api/company/companies/${companyId}/issue-labels`);
        let saw404 = false;
        for (const row of ISSUE_LABEL_SEED_DEFAULTS) {
          const cr = await fetch(createUrl, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({
              slug: row.slug,
              display_name: row.display_name,
              sort_order: row.sort_order,
            }),
          });
          if (cr.status === 404) {
            saw404 = true;
            break;
          }
          if (!cr.ok && cr.status !== 409) {
            const cj = (await cr.json().catch(() => ({}))) as { error?: string };
            throw new Error(cj.error ?? `Could not create label ${row.slug} (${cr.status})`);
          }
        }
        if (saw404) {
          return { skipped: true };
        }
        return;
      }

      throw new Error(j.error ?? `Load labels failed (${r.status})`);
    },
    onSuccess: (data) => {
      if (data && typeof data === "object" && data.skipped) {
        setLabelsCatalogSkipped(true);
        return;
      }
      setLabelsCatalogSkipped(false);
      void qc.invalidateQueries({ queryKey: ["hsm", "issue-labels", apiBase, companyId] });
    },
  });

  const createIssueLabel = useMutation({
    mutationFn: async (vars: { slug: string; display_name: string }) => {
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/issue-labels`), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(vars),
      });
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) {
        if (r.status === 404) {
          throw new Error("label_catalog_unavailable");
        }
        throw new Error(j.error ?? `${r.status}`);
      }
      return j;
    },
    onError: (e) => {
      if (e instanceof Error && e.message === "label_catalog_unavailable") {
        setLabelsCatalogSkipped(true);
      }
    },
    onSuccess: (_data, vars) => {
      setLabelsCatalogSkipped(false);
      void qc.invalidateQueries({ queryKey: ["hsm", "issue-labels", apiBase, companyId] });
      const slug = vars.slug.trim().toLowerCase();
      if (slug) setSelectedLabels((prev) => (prev.includes(slug) ? prev : [...prev, slug]));
      setNewLabelSlug("");
      setNewLabelName("");
      setShowAddLabel(false);
    },
  });

  const createTask = useMutation({
    mutationFn: async () => {
      const t = title.trim();
      if (!t) throw new Error("Title is required.");
      const paths = allWorkspacePaths.map((p) => p.trim()).filter(Boolean);
      let specification = specificationWithWorkspacePaths(bodyExtra, paths);
      if (labelsDeferredToReviewer) {
        const note =
          "\n\n---\n**Labels:** Pending — reviewer to assign when triaging.";
        specification = (specification.trimEnd() + note).trim();
      }
      const priorityNum =
        priorityTier === "reviewer" ? 0 : priorityTier === "low" ? 3 : priorityTier === "medium" ? 6 : 10;
      const payload: Record<string, unknown> = {
        title: t,
        specification: specification || null,
        workspace_attachment_paths: paths.length ? paths : undefined,
        capability_refs: buildCapabilityRefs(),
        priority: priorityNum,
      };
      const pid = projectId.trim();
      if (pid) payload.project_id = pid;
      if (assignToAgent && assigneePersona.trim()) {
        payload.owner_persona = assigneePersona.trim();
      }
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/tasks`), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(payload),
      });
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    onSuccess: () => {
      setTitle("");
      setBodyExtra("");
      setUserPaths([]);
      setPathDraft("");
      setRemovedSeed(new Set());
      setProjectId("");
      setPriorityTier("medium");
      setSelectedLabels([]);
      setLabelsDeferredToReviewer(false);
      setShowAddLabel(false);
      setNewLabelSlug("");
      setNewLabelName("");
      setAssignToAgent(Boolean(defaultPersona));
      setIssueKind("todo");
      setReviewerPersona(defaultPersona);
      setApproverPersona(defaultPersona);
      setRecurring("none");
      setRecurringOpen(false);
      try {
        localStorage.removeItem(draftKey(companyId));
      } catch {
        /* ignore */
      }
      void qc.invalidateQueries({ queryKey: ["hsm", "tasks", apiBase, companyId] });
      void qc.invalidateQueries({ queryKey: ["hsm", "intelligence", apiBase, companyId] });
      onCreated?.();
    },
  });

  const primaryPath = allWorkspacePaths[0]?.trim() ?? "";
  const showBanner = showAttachBanner && allWorkspacePaths.length > 0;

  const canSubmit = title.trim().length > 0 && !createTask.isPending;

  const recurringLabel =
    recurring === "none"
      ? "One-time"
      : recurring === "daily"
        ? "Daily"
        : recurring === "weekly"
          ? "Weekly"
          : "Monthly";

  return (
    <div className="space-y-4">
      <div
        className="space-y-3 rounded-lg border border-border bg-muted/20 p-3"
        role="group"
        aria-label="Issue type"
      >
        <div className="space-y-1">
          <Label className="text-xs font-medium text-foreground">Issue type</Label>
          <p className="text-[11px] text-muted-foreground">
            Choose <span className="font-medium text-foreground/90">Plan</span> (design first) or{" "}
            <span className="font-medium text-foreground/90">Task</span> (executable work) before you fill in the rest.
          </p>
        </div>
        <div className="flex flex-wrap gap-2" role="radiogroup" aria-label="Plan or task">
          <Button
            type="button"
            size="sm"
            variant="outline"
            role="radio"
            aria-checked={issueKind === "plan"}
            className={cn(
              "gap-1.5 px-4 py-2 font-medium",
              issueKind === "plan" &&
                "border-sky-500/70 bg-sky-500/15 text-sky-100 ring-2 ring-sky-500/50 dark:border-sky-400/60 dark:bg-sky-500/10",
            )}
            onClick={() => setIssueKind("plan")}
          >
            <Map className="size-3.5 opacity-90" aria-hidden />
            Plan
          </Button>
          <Button
            type="button"
            size="sm"
            variant="outline"
            role="radio"
            aria-checked={issueKind === "todo"}
            className={cn(
              "gap-1.5 px-4 py-2 font-medium",
              issueKind === "todo" &&
                "border-sky-500/70 bg-sky-500/15 text-sky-100 ring-2 ring-sky-500/50 dark:border-sky-400/60 dark:bg-sky-500/10",
            )}
            onClick={() => setIssueKind("todo")}
          >
            <CheckCircle2 className="size-3.5 opacity-90" aria-hidden />
            Task
          </Button>
        </div>
        <p className="text-[11px] leading-snug text-muted-foreground">
          {issueKind === "plan"
            ? "Plan — outline approach and sequencing; use Build on the task row to spawn implementation work when the plan is done."
            : "Task — normal work item for an agent to execute."}
        </p>
      </div>

      {showPathAttach ? (
        <div className="space-y-2">
          <Label className="text-xs text-muted-foreground" htmlFor={`${idPrefix}-ws-path`}>
            Attach from workspace
          </Label>
          <p className="text-[11px] text-muted-foreground">
            Relative to company <span className="font-mono text-[10px]">hsmii_home</span>. Press Enter or click Attach.
          </p>
          <div className="grid gap-2 sm:grid-cols-[1fr_auto] sm:items-end">
            <Input
              id={`${idPrefix}-ws-path`}
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
            <Button type="button" variant="secondary" className="w-full sm:w-auto" onClick={attachWorkspacePath}>
              Attach path
            </Button>
          </div>
          {allWorkspacePaths.length > 0 ? (
            <div className="flex flex-wrap gap-1">
              {allWorkspacePaths.map((p) => (
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
                    onClick={() => removeAttachedPath(p)}
                  >
                    ×
                  </button>
                </span>
              ))}
            </div>
          ) : null}
        </div>
      ) : null}

      {showBanner ? (
        <div
          className="flex gap-2 rounded-md border border-amber-500/45 bg-amber-500/10 px-3 py-2 text-sm text-amber-950 dark:border-amber-400/35 dark:bg-amber-400/10 dark:text-amber-50"
          role="status"
        >
          <Paperclip className="mt-0.5 h-4 w-4 shrink-0 opacity-80" aria-hidden />
          <div>
            <p className="font-medium">Workspace paths on this issue</p>
            <p className="mt-0.5 font-mono text-xs opacity-90">
              {allWorkspacePaths.length === 1
                ? truncatePath(primaryPath, 64)
                : `${allWorkspacePaths.length} paths · ${truncatePath(primaryPath, 48)}`}
            </p>
          </div>
        </div>
      ) : null}

      <div className="space-y-2">
        <Label htmlFor={`${idPrefix}-title`}>Title</Label>
        <Input
          id={`${idPrefix}-title`}
          placeholder="Issue title"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          className="text-base"
        />
      </div>

      <div className="relative z-10 flex flex-wrap items-center gap-2">
        <Button
          type="button"
          variant={assignToAgent ? "secondary" : "outline"}
          size="sm"
          className={cn("gap-1.5 font-normal", assignToAgent && "ring-1 ring-border/80")}
          onClick={(e) => {
            e.stopPropagation();
            setAssignToAgent((v) => !v);
          }}
          onPointerDown={(e) => e.stopPropagation()}
          title="Assign the issue to this workspace agent (sets owner_persona)"
        >
          <User className="size-3.5 opacity-80" aria-hidden />
          {assignToAgent ? `For ${assigneeDisplayName}` : "Assign to agent"}
        </Button>
      </div>

      <div className="space-y-3">
        <p className="text-xs font-medium text-muted-foreground">Project, priority &amp; labels</p>

        <div className="grid gap-3 sm:grid-cols-2">
          <div className="space-y-1.5">
            <Label className="text-xs text-foreground" htmlFor={`${idPrefix}-project`}>
              Project
            </Label>
            <Select
              value={projectId || "__none__"}
              onValueChange={(v) => setProjectId(v === "__none__" ? "" : v)}
            >
              <SelectTrigger id={`${idPrefix}-project`} className="h-9 text-sm">
                <SelectValue placeholder="No project" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none__" className="text-sm">
                  No project
                </SelectItem>
                {projectsSorted.map((p) => (
                  <SelectItem key={p.id} value={p.id} className="text-sm">
                    {p.title}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {projectsSorted.length === 0 ? (
              <p className="text-[11px] text-muted-foreground">No projects yet — add one from the console home.</p>
            ) : null}
          </div>

          <div className="space-y-1.5">
            <Label className="text-xs text-foreground">Priority</Label>
            <div className="flex flex-wrap gap-1" role="group" aria-label="Urgency">
              {(["low", "medium", "high"] as const).map((p) => {
                const active = priorityTier === p;
                return (
                  <Button
                    key={p}
                    type="button"
                    size="sm"
                    variant={active ? "secondary" : "outline"}
                    disabled={priorityTier === "reviewer"}
                    className={cn(
                      "h-8 min-w-[4.5rem] px-2.5 text-xs capitalize",
                      priorityTier === "reviewer" && "opacity-50",
                    )}
                    onClick={() => setPriorityTier(p)}
                  >
                    {p}
                  </Button>
                );
              })}
            </div>
            <button
              type="button"
              className={cn(
                "w-full rounded-md px-2 py-1.5 text-left text-[11px] transition-colors",
                priorityTier === "reviewer"
                  ? "bg-muted text-foreground"
                  : "text-muted-foreground hover:bg-muted/60 hover:text-foreground",
              )}
              onClick={() => setPriorityTier("reviewer")}
            >
              Reviewer sets urgency before dispatch
            </button>
          </div>
        </div>

        <div className="border-t border-border/40 pt-3">
          <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
            <span className="text-xs text-muted-foreground">
              Labels
              {selectedLabels.length > 0 && !labelsDeferredToReviewer ? (
                <span className="ml-1.5 tabular-nums text-foreground">({selectedLabels.length})</span>
              ) : null}
            </span>
            <div className="flex items-center gap-1">
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-7 px-2 text-xs"
                disabled={seedDefaultLabels.isPending || labelsCatalogSkipped || labelsDeferredToReviewer}
                onClick={() => seedDefaultLabels.mutate()}
              >
                <Sparkles className="mr-1 size-3 opacity-70" aria-hidden />
                {seedDefaultLabels.isPending ? "…" : "Suggested"}
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-7 px-2 text-xs"
                disabled={labelsCatalogSkipped || labelsDeferredToReviewer}
                onClick={() => setShowAddLabel((v) => !v)}
              >
                <Plus className="mr-1 size-3" aria-hidden />
                {showAddLabel ? "Close" : "New"}
              </Button>
            </div>
          </div>

          <div className="mb-3 flex gap-2.5">
            <Checkbox
              id={`${idPrefix}-labels-defer`}
              className="mt-0.5"
              checked={labelsDeferredToReviewer}
              onCheckedChange={(v) => {
                const on = v === true;
                if (on) {
                  setSelectedLabels([]);
                  setShowAddLabel(false);
                }
                setLabelsDeferredToReviewer(on);
              }}
            />
            <label
              htmlFor={`${idPrefix}-labels-defer`}
              className="min-w-0 cursor-pointer text-[11px] leading-snug text-muted-foreground"
            >
              <span className="font-medium text-foreground">Reviewer will assign labels</span>
              {" — "}Skip tags for now. We add a short note to the description and a small marker on the task so
              whoever triages it knows labels are still open.
            </label>
          </div>

          {labelsCatalogSkipped ? (
            <p className="mb-2 text-[11px] text-muted-foreground">
              Label catalog isn’t available on this server. You can still create the issue; your admin can enable labels by
              updating the database and console.
            </p>
          ) : null}

          {issueLabels.length > 0 ? (
            <div
              className={cn(
                "max-h-32 overflow-y-auto",
                labelsDeferredToReviewer && "pointer-events-none opacity-45",
              )}
              aria-disabled={labelsDeferredToReviewer}
            >
              <div className="flex flex-wrap gap-1">
                {issueLabels.map((l) => {
                  const on = selectedLabels.includes(l.slug);
                  return (
                    <button
                      key={l.id}
                      type="button"
                      title={l.description ?? l.slug}
                      disabled={labelsDeferredToReviewer}
                      onClick={() => toggleLabelSlug(l.slug)}
                      className={cn(
                        "max-w-full truncate rounded-md px-2 py-1 text-[11px] transition-colors",
                        on ? "bg-muted font-medium text-foreground" : "text-muted-foreground hover:bg-muted/70",
                      )}
                    >
                      {l.display_name}
                    </button>
                  );
                })}
              </div>
            </div>
          ) : !labelsCatalogSkipped ? (
            <p className="text-[11px] text-muted-foreground">
              No labels yet — try <span className="text-foreground/90">Suggested</span> or add your own.
            </p>
          ) : null}

          {seedDefaultLabels.isError ? (
            <p className="text-[11px] text-destructive">
              {seedDefaultLabels.error instanceof Error
                ? seedDefaultLabels.error.message
                : String(seedDefaultLabels.error)}
            </p>
          ) : null}

          {showAddLabel && !labelsCatalogSkipped && !labelsDeferredToReviewer ? (
            <div className="mt-2 space-y-2 rounded-md bg-muted/40 p-2">
              <div className="flex flex-col gap-2 sm:flex-row sm:items-end">
                <div className="min-w-0 flex-1 space-y-0.5">
                  <Label className="text-[10px] text-muted-foreground" htmlFor={`${idPrefix}-new-label-slug`}>
                    Slug
                  </Label>
                  <Input
                    id={`${idPrefix}-new-label-slug`}
                    className="h-8 text-xs"
                    placeholder="customer_escalation"
                    value={newLabelSlug}
                    onChange={(e) => setNewLabelSlug(e.target.value)}
                  />
                </div>
                <div className="min-w-0 flex-1 space-y-0.5">
                  <Label className="text-[10px] text-muted-foreground" htmlFor={`${idPrefix}-new-label-name`}>
                    Name
                  </Label>
                  <Input
                    id={`${idPrefix}-new-label-name`}
                    className="h-8 text-sm"
                    placeholder="Customer escalation"
                    value={newLabelName}
                    onChange={(e) => setNewLabelName(e.target.value)}
                  />
                </div>
                <Button
                  type="button"
                  size="sm"
                  className="h-8 shrink-0"
                  disabled={
                    createIssueLabel.isPending || !newLabelSlug.trim() || !newLabelName.trim()
                  }
                  onClick={() =>
                    createIssueLabel.mutate({
                      slug: newLabelSlug.trim(),
                      display_name: newLabelName.trim(),
                    })
                  }
                >
                  {createIssueLabel.isPending ? "…" : "Save"}
                </Button>
              </div>
            </div>
          ) : null}
          {createIssueLabel.isError ? (
            <p className="text-[11px] text-destructive">
              {createIssueLabel.error instanceof Error &&
              createIssueLabel.error.message === "label_catalog_unavailable"
                ? "Label catalog isn’t enabled on this server yet."
                : createIssueLabel.error instanceof Error
                  ? createIssueLabel.error.message
                  : String(createIssueLabel.error)}
            </p>
          ) : null}
        </div>
      </div>

      <div className="space-y-2">
        <Label htmlFor={`${idPrefix}-body`}>Description</Label>
        <Textarea
          id={`${idPrefix}-body`}
          className="min-h-[160px] font-mono text-xs"
          placeholder="Context, acceptance criteria, links…"
          value={bodyExtra}
          onChange={(e) => setBodyExtra(e.target.value)}
        />
        {primaryPath ? (
          <p className="font-mono text-[11px] text-muted-foreground">
            {specificationWithWorkspacePaths("", [primaryPath]).trim()}
          </p>
        ) : null}
      </div>

      <Collapsible open={advancedOpen} onOpenChange={setAdvancedOpen}>
        <CollapsibleTrigger
          type="button"
          className="flex w-full items-center gap-1 rounded-md border border-border bg-muted/30 px-3 py-2 text-left text-sm font-medium hover:bg-muted/50 [&[data-state=open]>svg]:rotate-180"
        >
          <ChevronDown className="size-4 shrink-0 transition-transform" aria-hidden />
          Reviewers &amp; recurrence
        </CollapsibleTrigger>
        <CollapsibleContent className="space-y-4 border-l-2 border-primary/25 pl-3 pt-3 data-[state=closed]:pointer-events-none">
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="space-y-1.5">
              <Label className="text-xs">Reviewer</Label>
              <p className="text-[10px] text-muted-foreground">
                Defaults to this workspace agent; pick who should review (any company agent).
              </p>
              <Select
                value={reviewerPersona || "__none__"}
                onValueChange={(v) => setReviewerPersona(v === "__none__" ? "" : v)}
              >
                <SelectTrigger className="font-mono text-xs">
                  <SelectValue placeholder="No reviewer" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__none__" className="font-mono text-xs">
                    No reviewer
                  </SelectItem>
                  {reviewerPersona && !agents.some((a) => a.name === reviewerPersona) ? (
                    <SelectItem value={reviewerPersona} className="font-mono text-xs">
                      {reviewerPersona} (current)
                    </SelectItem>
                  ) : null}
                  {agents.map((a) => (
                    <SelectItem key={a.id} value={a.name} className="font-mono text-xs">
                      {a.name}
                      {a.title ? ` — ${a.title}` : ""}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <Label className="text-xs">Approver</Label>
              <p className="text-[10px] text-muted-foreground">
                Who signs off — defaults to the agent in charge; choose another if needed.
              </p>
              <Select
                value={approverPersona || "__none__"}
                onValueChange={(v) => setApproverPersona(v === "__none__" ? "" : v)}
              >
                <SelectTrigger className="font-mono text-xs">
                  <SelectValue placeholder="No approver" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__none__" className="font-mono text-xs">
                    No approver
                  </SelectItem>
                  {approverPersona && !agents.some((a) => a.name === approverPersona) ? (
                    <SelectItem value={approverPersona} className="font-mono text-xs">
                      {approverPersona} (current)
                    </SelectItem>
                  ) : null}
                  {agents.map((a) => (
                    <SelectItem key={a.id} value={a.name} className="font-mono text-xs">
                      {a.name}
                      {a.title ? ` — ${a.title}` : ""}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>

          <Collapsible open={recurringOpen} onOpenChange={setRecurringOpen}>
            <CollapsibleTrigger className="flex w-full items-center gap-2 rounded-md border border-border/80 bg-muted/20 px-3 py-2 text-left text-sm font-medium hover:bg-muted/40 [&[data-state=open]>svg]:rotate-180">
              <ChevronDown className="size-4 shrink-0 transition-transform" aria-hidden />
              <Repeat className="size-4 shrink-0 text-muted-foreground" aria-hidden />
              <span>Recurring task</span>
              <span className="ml-auto font-mono text-[11px] font-normal text-muted-foreground">{recurringLabel}</span>
            </CollapsibleTrigger>
            <CollapsibleContent className="mt-2 space-y-2 pl-1 data-[state=closed]:pointer-events-none">
              <Select value={recurring} onValueChange={(v) => setRecurring(v as Recurring)}>
                <SelectTrigger className="max-w-xs font-mono text-xs">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="none">One-time (not recurring)</SelectItem>
                  <SelectItem value="daily">Daily</SelectItem>
                  <SelectItem value="weekly">Weekly</SelectItem>
                  <SelectItem value="monthly">Monthly</SelectItem>
                </SelectContent>
              </Select>
              <p className="text-[10px] text-muted-foreground">
                Stored as <span className="font-mono">mode: recurring:…</span> on the task; your scheduler integration
                enforces cadence.
              </p>
            </CollapsibleContent>
          </Collapsible>
        </CollapsibleContent>
      </Collapsible>

      {createTask.isError ? (
        <p className="text-sm text-destructive">
          {createTask.error instanceof Error ? createTask.error.message : String(createTask.error)}
        </p>
      ) : null}

      <div className="flex flex-col gap-3 border-t border-border/60 pt-4 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex flex-wrap gap-2">
          {onCloseRequest ? (
            <Button type="button" variant="ghost" size="sm" className="text-muted-foreground" onClick={onCloseRequest}>
              Cancel
            </Button>
          ) : null}
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="text-muted-foreground"
            onClick={() => {
              setTitle("");
              setBodyExtra("");
              setUserPaths([]);
              setPathDraft("");
              setRemovedSeed(new Set());
              setProjectId("");
              setPriorityTier("medium");
              setSelectedLabels([]);
              setShowAddLabel(false);
              setNewLabelSlug("");
              setNewLabelName("");
              setReviewerPersona(defaultPersona);
              setApproverPersona(defaultPersona);
              setRecurring("none");
              try {
                localStorage.removeItem(draftKey(companyId));
              } catch {
                /* ignore */
              }
            }}
          >
            Discard draft
          </Button>
        </div>
        <div className="flex flex-1 flex-wrap items-center justify-end gap-4">
          <span className="text-[11px] text-muted-foreground">Draft autosaves locally</span>
          <Button
            type="button"
            className="min-w-[140px] bg-orange-600 text-white hover:bg-orange-600/90 dark:bg-orange-600 dark:hover:bg-orange-600/90"
            disabled={!canSubmit}
            onClick={() => createTask.mutate(undefined)}
          >
            {createTask.isPending ? "Creating…" : "Create issue"}
          </Button>
        </div>
      </div>
    </div>
  );
}
