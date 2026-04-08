"use client";

import { Dispatch, SetStateAction, useEffect, useMemo, useState } from "react";
import { friendlyTaskState } from "../lib/inboxPlainLanguage";
import { StatusChip, type ChipTone } from "./StatusChip";

type TaskRow = {
  id: string;
  title: string;
  state: string;
  specification?: string | null;
  project_id?: string | null;
  checked_out_by?: string | null;
  checked_out_until?: string | null;
  owner_persona?: string | null;
  due_at?: string | null;
  sla_policy?: string | null;
  priority?: number;
  requires_human?: boolean;
  /** JSON array from API — paths relative to company hsmii_home */
  workspace_attachment_paths?: unknown;
  /** JSON array of `{ kind, ref }` from Company OS */
  capability_refs?: unknown;
  /** From task_run_snapshots merged into list tasks API */
  run?: {
    status: string;
    tool_calls: number;
    log_tail: string;
    finished_at?: string | null;
    updated_at?: string;
  } | null;
};

export function workspaceAttachmentPathsFromTask(t: { workspace_attachment_paths?: unknown }): string[] {
  const v = t.workspace_attachment_paths;
  if (!Array.isArray(v)) return [];
  return v.filter((x): x is string => typeof x === "string" && x.trim().length > 0).map((s) => s.trim());
}

export function capabilityRefsFromTask(t: { capability_refs?: unknown }): { kind: string; ref: string }[] {
  const v = t.capability_refs;
  if (!Array.isArray(v)) return [];
  const out: { kind: string; ref: string }[] = [];
  for (const x of v) {
    if (x && typeof x === "object" && !Array.isArray(x)) {
      const o = x as Record<string, unknown>;
      const ref = typeof o.ref === "string" ? o.ref.trim() : "";
      if (!ref) continue;
      const kind = typeof o.kind === "string" && o.kind.trim() ? o.kind.trim() : "skill";
      out.push({ kind, ref });
    }
  }
  return out;
}

/** Narrowing applied when drilling in from the Dashboard charts/metrics. */
export type TaskListDashboardFilter =
  | { kind: "priority"; level: number }
  | { kind: "state"; state: string }
  | { kind: "ids"; ids: string[] }
  | { kind: "in_progress" }
  | { kind: "open" }
  | { kind: "blocked" }
  | { kind: "completed" };

type GovEvent = {
  actor: string;
  payload?: unknown;
  created_at: string;
};

type Props = {
  api: string;
  coTasks: TaskRow[];
  coCheckoutAgent: string;
  coLatestTaskDecision: Map<string, GovEvent>;
  coSlaDueAt: Record<string, string>;
  setCoSlaDueAt: Dispatch<SetStateAction<Record<string, string>>>;
  coSlaEscAt: Record<string, string>;
  setCoSlaEscAt: Dispatch<SetStateAction<Record<string, string>>>;
  coSlaPol: Record<string, string>;
  setCoSlaPol: Dispatch<SetStateAction<Record<string, string>>>;
  coSlaPrio: Record<string, string>;
  setCoSlaPrio: Dispatch<SetStateAction<Record<string, string>>>;
  coSlaReason: Record<string, string>;
  setCoSlaReason: Dispatch<SetStateAction<Record<string, string>>>;
  setCoErr: Dispatch<SetStateAction<string | null>>;
  loadCompanyOs: () => Promise<void>;
  /** When set, only tasks in this Paperclip-style project */
  filterProjectId?: string | null;
  onClearProjectFilter?: () => void;
  /** id → title for project chips */
  projectTitles?: Record<string, string>;
  /** When set, only tasks owned by or checked out to this persona */
  filterPersona?: string | null;
  onClearPersonaFilter?: () => void;
  dashboardFilter?: TaskListDashboardFilter | null;
  onClearDashboardFilter?: () => void;
  scrollToTaskId?: string | null;
  onScrollToTaskDone?: () => void;
};

export function TaskListPanel(props: Props) {
  const [workspacePathDraft, setWorkspacePathDraft] = useState<Record<string, string>>({});
  const [capabilityRefsDraft, setCapabilityRefsDraft] = useState<Record<string, string>>({});
  const [taskRunMsgDraft, setTaskRunMsgDraft] = useState<Record<string, string>>({});

  const {
    api,
    coTasks,
    coCheckoutAgent,
    coLatestTaskDecision,
    coSlaDueAt,
    setCoSlaDueAt,
    coSlaEscAt,
    setCoSlaEscAt,
    coSlaPol,
    setCoSlaPol,
    coSlaPrio,
    setCoSlaPrio,
    coSlaReason,
    setCoSlaReason,
    setCoErr,
    loadCompanyOs,
    filterProjectId = null,
    onClearProjectFilter,
    projectTitles = {},
    filterPersona = null,
    onClearPersonaFilter,
    dashboardFilter = null,
    onClearDashboardFilter,
    scrollToTaskId = null,
    onScrollToTaskDone,
  } = props;

  const [highlightTaskId, setHighlightTaskId] = useState<string | null>(null);

  const projectId = filterProjectId?.trim() ?? "";
  const persona = filterPersona?.trim() ?? "";
  const visibleTasks = useMemo(() => {
    let list = coTasks;
    if (projectId) {
      list = list.filter((t) => (t.project_id ?? "").trim() === projectId);
    }
    if (persona) {
      list = list.filter(
        (t) => (t.owner_persona ?? "").trim() === persona || (t.checked_out_by ?? "").trim() === persona
      );
    }
    if (dashboardFilter) {
      switch (dashboardFilter.kind) {
        case "priority":
          list = list.filter((t) => (typeof t.priority === "number" ? t.priority : 1) === dashboardFilter.level);
          break;
        case "state":
          list = list.filter((t) => t.state === dashboardFilter.state);
          break;
        case "ids": {
          const set = new Set(dashboardFilter.ids);
          list = list.filter((t) => set.has(t.id));
          break;
        }
        case "in_progress":
          list = list.filter((t) => /progress|doing|active/i.test(t.state) || !!t.checked_out_by);
          break;
        case "open":
          list = list.filter((t) => /open|todo|pending/i.test(t.state));
          break;
        case "blocked":
          list = list.filter((t) => /block/i.test(t.state));
          break;
        case "completed":
          list = list.filter((t) => /done|complete|close/i.test(t.state));
          break;
        default:
          break;
      }
    }
    return list;
  }, [coTasks, projectId, persona, dashboardFilter]);

  useEffect(() => {
    if (!scrollToTaskId) return;
    const id = scrollToTaskId;
    const run = () => {
      const el = document.querySelector(`[data-task-row="${id}"]`);
      el?.scrollIntoView({ behavior: "smooth", block: "center" });
      onScrollToTaskDone?.();
    };
    const t = window.setTimeout(run, 50);
    setHighlightTaskId(id);
    const clearH = window.setTimeout(() => setHighlightTaskId(null), 2600);
    return () => {
      window.clearTimeout(t);
      window.clearTimeout(clearH);
    };
  }, [scrollToTaskId, onScrollToTaskDone]);

  const dashboardFilterLabel = dashboardFilter
    ? dashboardFilter.kind === "priority"
      ? `Priority ${dashboardFilter.level}`
      : dashboardFilter.kind === "state"
        ? `Status ${dashboardFilter.state}`
        : dashboardFilter.kind === "ids"
          ? `${dashboardFilter.ids.length} tasks from chart`
          : dashboardFilter.kind === "in_progress"
            ? "In progress"
            : dashboardFilter.kind === "open"
              ? "Open"
              : dashboardFilter.kind === "blocked"
                ? "Blocked"
                : "Completed"
    : null;

  function taskChipTone(t: TaskRow): ChipTone {
    if (t.state === "blocked") return "red";
    if (t.state === "waiting_admin") return "amber";
    if (/fail|error/i.test(t.state)) return "red";
    if (/progress|doing|active|open|todo|pending/i.test(t.state)) return "green";
    return "gray";
  }

  function taskChipLabel(t: TaskRow): string {
    if (t.state === "blocked") return "Blocked";
    if (t.state === "waiting_admin") return "Needs you";
    if (/fail|error/i.test(t.state)) return "Problem";
    if (/progress|doing|active/i.test(t.state)) return "In motion";
    return "Routine";
  }

  return (
    <div className="rounded-2xl border border-[#30363D] bg-[#0d1117]">
      <div className="border-b border-[#30363D] px-4 py-4">
        <p className="font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-[#8B949E]">Issues</p>
        <div className="mt-1 flex flex-wrap items-start justify-between gap-2">
          <h2 className="text-base font-semibold text-white">
            {projectId && projectTitles[projectId]
              ? `Tasks in ${projectTitles[projectId]}`
              : persona
                ? `Tasks for ${persona}`
                : "All tasks"}
          </h2>
          <div className="flex flex-wrap gap-2">
            {projectId && onClearProjectFilter ? (
              <button
                type="button"
                className="shrink-0 rounded-md border border-[#30363D] px-3 py-1 font-mono text-[11px] uppercase tracking-wide text-[#8B949E] hover:border-[#484f58] hover:text-[#c9d1d9]"
                onClick={onClearProjectFilter}
              >
                Clear project filter
              </button>
            ) : null}
            {dashboardFilter && onClearDashboardFilter ? (
              <button
                type="button"
                className="shrink-0 rounded-md border border-[#58a6ff]/40 bg-[#388bfd]/10 px-3 py-1 font-mono text-[11px] uppercase tracking-wide text-[#58a6ff] hover:bg-[#388bfd]/20"
                onClick={onClearDashboardFilter}
              >
                Clear chart filter
              </button>
            ) : null}
            {persona && onClearPersonaFilter ? (
              <button
                type="button"
                className="shrink-0 rounded-md border border-[#30363D] px-3 py-1 font-mono text-[11px] uppercase tracking-wide text-[#8B949E] hover:border-[#484f58] hover:text-[#c9d1d9]"
                onClick={onClearPersonaFilter}
              >
                Show all tasks
              </button>
            ) : null}
          </div>
        </div>
        {dashboardFilterLabel ? (
          <p className="mt-2 font-mono text-[11px] text-[#58a6ff]">Dashboard filter: {dashboardFilterLabel}</p>
        ) : null}
        <p className="mt-1 text-sm leading-relaxed text-[#8B949E]">
          {projectId
            ? "Issues filed under this project. Create tasks from the Tasks tab and pick a project, or use the sidebar to jump here."
            : persona
              ? "Work this role owns or has checked out. Use Assign / Hand back below to change who is active on a task."
              : "Operational backlog: checkout, SLA, assignment. Approvals and agent escalations are handled in Inbox, not here."}
        </p>
      </div>
      <ul className="divide-y divide-[#30363D]">
        {visibleTasks.map((t) => {
          const capRefs = capabilityRefsFromTask(t);
          return (
          <li
            key={t.id}
            data-task-row={t.id}
            className={`px-4 py-3 text-sm transition-shadow duration-500 ${
              highlightTaskId === t.id ? "ring-2 ring-[#58a6ff]/80 ring-offset-2 ring-offset-[#0d1117]" : ""
            }`}
          >
            <div className="flex flex-wrap items-start justify-between gap-2">
              <div>
                <div className="font-medium text-gray-200">{t.title}</div>
                <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-gray-500">
                  <span>Status: {friendlyTaskState(t.state)}</span>
                  {t.run ? (
                    <span className="rounded border border-[#388bfd]/35 bg-[#388bfd]/10 px-1.5 py-0.5 font-mono text-[10px] text-[#79b8ff]">
                      run: {t.run.status} · {t.run.tool_calls} tools
                    </span>
                  ) : null}
                  <StatusChip label={taskChipLabel(t)} tone={taskChipTone(t)} />
                  {t.requires_human ? (
                    <StatusChip label="Human inbox" tone="amber" />
                  ) : null}
                  {workspaceAttachmentPathsFromTask(t).length ? (
                    <StatusChip label="Workspace file attached" tone="green" />
                  ) : null}
                  {capRefs.length ? (
                    <StatusChip label={`${capRefs.length} capability link(s)`} tone="green" />
                  ) : null}
                  {t.project_id && projectTitles[(t.project_id ?? "").trim()] ? (
                    <span className="text-[#58a6ff]">Project: {projectTitles[(t.project_id ?? "").trim()]}</span>
                  ) : null}
                  {t.owner_persona ? <span>Owner: {t.owner_persona}</span> : null}
                  {t.checked_out_by ? <span>With: {t.checked_out_by}</span> : null}
                  {t.checked_out_until ? <span>Until {String(t.checked_out_until)}</span> : null}
                  {t.due_at ? <span>Due {String(t.due_at)}</span> : null}
                  {t.sla_policy ? <span>Policy: {t.sla_policy}</span> : null}
                </div>
                {workspaceAttachmentPathsFromTask(t).length ? (
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    {workspaceAttachmentPathsFromTask(t).map((p) => (
                      <span
                        key={p}
                        className="max-w-full truncate rounded border border-[#388bfd]/35 bg-[#388bfd]/10 px-2 py-0.5 font-mono text-[10px] text-[#79b8ff]"
                        title={p}
                      >
                        {p}
                      </span>
                    ))}
                  </div>
                ) : null}
                {capRefs.length ? (
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    {capRefs.map((c) => (
                      <span
                        key={`${c.kind}:${c.ref}`}
                        className="max-w-full truncate rounded border border-emerald-500/35 bg-emerald-500/10 px-2 py-0.5 font-mono text-[10px] text-emerald-200/90"
                        title={`${c.kind}: ${c.ref}`}
                      >
                        {c.kind}:{c.ref.length > 28 ? `${c.ref.slice(0, 26)}…` : c.ref}
                      </span>
                    ))}
                  </div>
                ) : null}
              </div>
              <div className="flex shrink-0 gap-2">
                <button
                  type="button"
                  title="Let an agent or automation pick this up"
                  className="rounded-full border border-primary/50 bg-primary px-3 py-1.5 text-xs font-semibold text-primary-foreground shadow-sm hover:bg-primary/90"
                  onClick={async () => {
                    setCoErr(null);
                    try {
                      const r = await fetch(`${api}/api/company/tasks/${t.id}/checkout`, {
                        method: "POST",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({
                          agent_ref: coCheckoutAgent.trim() || "agent",
                        }),
                      });
                      const j = await r.json();
                      if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                      await loadCompanyOs();
                    } catch (e) {
                      setCoErr(e instanceof Error ? e.message : String(e));
                    }
                  }}
                >
                  Assign
                </button>
                <button
                  type="button"
                  title="Put this task back in the pool"
                  className="rounded-full border border-line px-3 py-1.5 text-xs font-medium text-gray-400 hover:bg-white/5"
                  onClick={async () => {
                    setCoErr(null);
                    try {
                      const r = await fetch(`${api}/api/company/tasks/${t.id}/release`, {
                        method: "POST",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({
                          actor: coCheckoutAgent.trim() || "console",
                        }),
                      });
                      const j = await r.json();
                      if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                      await loadCompanyOs();
                    } catch (e) {
                      setCoErr(e instanceof Error ? e.message : String(e));
                    }
                  }}
                >
                  Hand back
                </button>
                {!/done|closed|cancel/i.test(t.state) && !t.requires_human ? (
                  <button
                    type="button"
                    title="Agent escalation — appears in human Inbox for review"
                    className="rounded-full border border-amber-800/60 px-3 py-1.5 text-xs font-medium text-amber-200/90 hover:bg-amber-950/40"
                    onClick={async () => {
                      setCoErr(null);
                      const reason =
                        typeof window !== "undefined"
                          ? window.prompt("Why should a human look at this? (optional)") ?? ""
                          : "";
                      try {
                        const r = await fetch(`${api}/api/company/tasks/${t.id}/requires-human`, {
                          method: "POST",
                          headers: { "Content-Type": "application/json" },
                          body: JSON.stringify({
                            requires_human: true,
                            actor: coCheckoutAgent.trim() || "operator",
                            reason: reason.trim(),
                          }),
                        });
                        const j = await r.json();
                        if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                        await loadCompanyOs();
                      } catch (e) {
                        setCoErr(e instanceof Error ? e.message : String(e));
                      }
                    }}
                  >
                    Need human
                  </button>
                ) : null}
              </div>
            </div>
            {t.specification && (
              <pre className="mt-1 whitespace-pre-wrap font-mono text-[11px] text-gray-500">
                {t.specification}
              </pre>
            )}
            <details className="mt-2 rounded border border-line/60 bg-ink/20 px-2 py-1">
              <summary className="cursor-pointer select-none text-xs text-gray-500 hover:text-gray-400">
                Workspace paths (hsmii_home-relative)
              </summary>
              <p className="mt-2 text-[11px] leading-relaxed text-gray-600">
                Stable pointers for agents (Paperclip-style). One path per line; saved to the task record and included in
                LLM context.
              </p>
              <textarea
                className="mt-2 w-full rounded border border-line bg-ink px-2 py-1.5 font-mono text-[11px] text-gray-300"
                rows={3}
                placeholder="workspace/content/…"
                value={
                  workspacePathDraft[t.id] !== undefined
                    ? workspacePathDraft[t.id]!
                    : workspaceAttachmentPathsFromTask(t).join("\n")
                }
                onChange={(e) =>
                  setWorkspacePathDraft((m) => ({
                    ...m,
                    [t.id]: e.target.value,
                  }))
                }
              />
              <button
                type="button"
                className="mt-2 rounded border border-line px-2 py-1 text-xs text-gray-300 hover:bg-white/5"
                onClick={async () => {
                  setCoErr(null);
                  try {
                    const raw =
                      workspacePathDraft[t.id] !== undefined
                        ? workspacePathDraft[t.id]!
                        : workspaceAttachmentPathsFromTask(t).join("\n");
                    const paths = raw
                      .split(/\r?\n/)
                      .map((s) => s.trim())
                      .filter(Boolean);
                    const r = await fetch(`${api}/api/company/tasks/${t.id}/context`, {
                      method: "PATCH",
                      headers: { "Content-Type": "application/json" },
                      body: JSON.stringify({ workspace_attachment_paths: paths }),
                    });
                    const j = await r.json();
                    if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                    setWorkspacePathDraft((m) => {
                      const n = { ...m };
                      delete n[t.id];
                      return n;
                    });
                    await loadCompanyOs();
                  } catch (e) {
                    setCoErr(e instanceof Error ? e.message : String(e));
                  }
                }}
              >
                Save paths
              </button>
            </details>
            <details className="mt-2 rounded border border-emerald-500/25 bg-emerald-500/5 px-2 py-1">
              <summary className="cursor-pointer select-none text-xs text-emerald-200/90 hover:text-emerald-100">
                Capability links (skills / SOPs / tools / packs / agents)
              </summary>
              <p className="mt-2 text-[11px] leading-relaxed text-gray-600">
                JSON array, e.g. <code className="font-mono text-[10px]">[&quot;skill-slug&quot;, {`{ "kind": "sop", "ref": "hr/onboarding" }`}]</code>. Stored on the task and merged into{" "}
                <span className="font-mono text-[10px]">llm-context</span>.
              </p>
              <textarea
                className="mt-2 w-full rounded border border-line bg-ink px-2 py-1.5 font-mono text-[11px] text-gray-300"
                rows={4}
                placeholder='["example-skill", { "kind": "tool", "ref": "company_memory_search" }]'
                value={
                  capabilityRefsDraft[t.id] !== undefined
                    ? capabilityRefsDraft[t.id]!
                    : JSON.stringify(capRefs.length ? capRefs.map((c) => ({ kind: c.kind, ref: c.ref })) : [])
                }
                onChange={(e) =>
                  setCapabilityRefsDraft((m) => ({
                    ...m,
                    [t.id]: e.target.value,
                  }))
                }
              />
              <button
                type="button"
                className="mt-2 rounded border border-line px-2 py-1 text-xs text-gray-300 hover:bg-white/5"
                onClick={async () => {
                  setCoErr(null);
                  try {
                    const raw =
                      capabilityRefsDraft[t.id] !== undefined
                        ? capabilityRefsDraft[t.id]!
                        : JSON.stringify(capRefs.length ? capRefs.map((c) => ({ kind: c.kind, ref: c.ref })) : []);
                    const parsed = JSON.parse(raw) as unknown;
                    if (!Array.isArray(parsed)) throw new Error("capability_refs must be a JSON array");
                    const r = await fetch(`${api}/api/company/tasks/${t.id}/context`, {
                      method: "PATCH",
                      headers: { "Content-Type": "application/json" },
                      body: JSON.stringify({ capability_refs: parsed }),
                    });
                    const j = await r.json();
                    if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                    setCapabilityRefsDraft((m) => {
                      const n = { ...m };
                      delete n[t.id];
                      return n;
                    });
                    await loadCompanyOs();
                  } catch (e) {
                    setCoErr(e instanceof Error ? e.message : String(e));
                  }
                }}
              >
                Save capability links
              </button>
            </details>
            <details className="mt-2 rounded border border-[#388bfd]/25 bg-[#388bfd]/5 px-2 py-1">
              <summary className="cursor-pointer select-none text-xs text-[#79b8ff] hover:text-[#a5d6ff]">
                Run log &amp; handoff (Paperclip-style strip)
              </summary>
              {t.run?.log_tail ? (
                <pre className="mt-2 max-h-36 overflow-auto whitespace-pre-wrap rounded border border-line bg-ink/80 p-2 font-mono text-[10px] text-gray-400">
                  {t.run.log_tail}
                </pre>
              ) : (
                <p className="mt-2 text-[11px] text-gray-600">No run telemetry yet. Agents POST to /run-telemetry as they work.</p>
              )}
              <textarea
                className="mt-2 w-full rounded border border-line bg-ink px-2 py-1.5 font-mono text-[11px] text-gray-300"
                rows={2}
                placeholder="Message for next assignee, or a line to append to the run log…"
                value={taskRunMsgDraft[t.id] ?? ""}
                onChange={(e) =>
                  setTaskRunMsgDraft((m) => ({
                    ...m,
                    [t.id]: e.target.value,
                  }))
                }
              />
              <div className="mt-2 flex flex-wrap gap-2">
                <button
                  type="button"
                  className="rounded border border-line px-2 py-1 text-[11px] text-gray-300 hover:bg-white/5"
                  onClick={async () => {
                    const text = (taskRunMsgDraft[t.id] ?? "").trim();
                    if (!text) return;
                    setCoErr(null);
                    try {
                      const r = await fetch(`${api}/api/company/tasks/${t.id}/stigmergic-note`, {
                        method: "POST",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({ text, actor: coCheckoutAgent.trim() || "operator" }),
                      });
                      const j = await r.json();
                      if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                      setTaskRunMsgDraft((m) => {
                        const n = { ...m };
                        delete n[t.id];
                        return n;
                      });
                      await loadCompanyOs();
                    } catch (e) {
                      setCoErr(e instanceof Error ? e.message : String(e));
                    }
                  }}
                >
                  Append handoff note
                </button>
                <button
                  type="button"
                  className="rounded border border-[#388bfd]/40 px-2 py-1 text-[11px] text-[#79b8ff] hover:bg-[#388bfd]/10"
                  onClick={async () => {
                    const raw = (taskRunMsgDraft[t.id] ?? "").trim();
                    if (!raw) return;
                    setCoErr(null);
                    const log_append = `[operator:${coCheckoutAgent.trim() || "console"}] ${raw}\n`;
                    try {
                      const r = await fetch(`${api}/api/company/tasks/${t.id}/run-telemetry`, {
                        method: "POST",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({ log_append }),
                      });
                      const j = await r.json();
                      if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                      setTaskRunMsgDraft((m) => {
                        const n = { ...m };
                        delete n[t.id];
                        return n;
                      });
                      await loadCompanyOs();
                    } catch (e) {
                      setCoErr(e instanceof Error ? e.message : String(e));
                    }
                  }}
                >
                  Append to run log
                </button>
              </div>
            </details>
            {coLatestTaskDecision.get(t.id) && (
              <div className="mt-2">
                <span className="inline-flex items-center gap-1 rounded border border-line bg-ink/60 px-2 py-0.5 text-[10px] text-gray-400">
                  {(() => {
                    const ev = coLatestTaskDecision.get(t.id)!;
                    const p = (ev.payload ?? {}) as { decision_mode?: string; reason?: string };
                    const d = (p.decision_mode ?? "").toUpperCase() || "DECISION";
                    const rs = (p.reason ?? "").trim();
                    const reasonPart = rs ? ` · ${rs}` : "";
                    return `last ${d} by ${ev.actor} · ${ev.created_at}${reasonPart}`;
                  })()}
                </span>
              </div>
            )}
            <details className="mt-3 rounded border border-line/60 bg-ink/20 px-2 py-1">
              <summary className="cursor-pointer select-none text-xs text-gray-500 hover:text-gray-400">
                Timing &amp; priority (optional)
              </summary>
              <div className="mt-2 grid grid-cols-1 gap-2 md:grid-cols-5">
              <input
                className="rounded border border-line bg-ink px-2 py-1 text-xs"
                placeholder="Due date (ISO)"
                value={coSlaDueAt[t.id] ?? ""}
                onChange={(e) => setCoSlaDueAt((m) => ({ ...m, [t.id]: e.target.value }))}
              />
              <input
                className="rounded border border-line bg-ink px-2 py-1 text-xs"
                placeholder="Escalate if still open (ISO)"
                value={coSlaEscAt[t.id] ?? ""}
                onChange={(e) => setCoSlaEscAt((m) => ({ ...m, [t.id]: e.target.value }))}
              />
              <input
                className="rounded border border-line bg-ink px-2 py-1 text-xs"
                placeholder="SLA name"
                value={coSlaPol[t.id] ?? ""}
                onChange={(e) => setCoSlaPol((m) => ({ ...m, [t.id]: e.target.value }))}
              />
              <input
                className="rounded border border-line bg-ink px-2 py-1 text-xs"
                placeholder="Priority (number)"
                value={coSlaPrio[t.id] ?? ""}
                onChange={(e) => setCoSlaPrio((m) => ({ ...m, [t.id]: e.target.value }))}
              />
              <div className="flex gap-2">
                <input
                  className="min-w-0 flex-1 rounded border border-line bg-ink px-2 py-1 text-xs"
                  placeholder="Why (short note)"
                  value={coSlaReason[t.id] ?? ""}
                  onChange={(e) => setCoSlaReason((m) => ({ ...m, [t.id]: e.target.value }))}
                />
                <button
                  type="button"
                  className="rounded border border-line px-2 py-1 text-xs text-gray-300"
                  onClick={async () => {
                    setCoErr(null);
                    try {
                      const maybeIso = (v: string) => {
                        const txt = v.trim();
                        return txt ? txt : undefined;
                      };
                      const p = (coSlaPrio[t.id] ?? "").trim();
                      const prio = p ? Number(p) : undefined;
                      const r = await fetch(`${api}/api/company/tasks/${t.id}/sla`, {
                        method: "PATCH",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({
                          due_at: maybeIso(coSlaDueAt[t.id] ?? ""),
                          escalate_after: maybeIso(coSlaEscAt[t.id] ?? ""),
                          sla_policy: (coSlaPol[t.id] ?? "").trim() || undefined,
                          status_reason: (coSlaReason[t.id] ?? "").trim() || undefined,
                          priority: Number.isFinite(prio as number) ? prio : undefined,
                        }),
                      });
                      const j = await r.json();
                      if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                      await loadCompanyOs();
                    } catch (e) {
                      setCoErr(e instanceof Error ? e.message : String(e));
                    }
                  }}
                >
                  Save
                </button>
              </div>
              </div>
            </details>
          </li>
          );
        })}
        {!visibleTasks.length && (
          <li className="px-4 py-8 text-center text-sm text-gray-500">
            {persona
              ? `No tasks for ${persona} right now. Pick another agent in the sidebar or clear the filter.`
              : dashboardFilter
                ? "Nothing matches this dashboard filter. Clear the chart filter or pick another bar on the dashboard."
                : "No tasks yet. Add one above—think of it as a sticky note the whole team can see."}
          </li>
        )}
      </ul>
    </div>
  );
}

