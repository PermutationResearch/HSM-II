"use client";

import { Dispatch, SetStateAction, useEffect, useMemo, useState } from "react";
import { friendlyTaskState } from "../lib/inboxPlainLanguage";
import { StatusChip, type ChipTone } from "./StatusChip";

type TaskRow = {
  id: string;
  title: string;
  state: string;
  specification?: string | null;
  checked_out_by?: string | null;
  checked_out_until?: string | null;
  owner_persona?: string | null;
  due_at?: string | null;
  sla_policy?: string | null;
  priority?: number;
};

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
  /** When set, only tasks owned by or checked out to this persona */
  filterPersona?: string | null;
  onClearPersonaFilter?: () => void;
  dashboardFilter?: TaskListDashboardFilter | null;
  onClearDashboardFilter?: () => void;
  scrollToTaskId?: string | null;
  onScrollToTaskDone?: () => void;
};

export function TaskListPanel(props: Props) {
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
    filterPersona = null,
    onClearPersonaFilter,
    dashboardFilter = null,
    onClearDashboardFilter,
    scrollToTaskId = null,
    onScrollToTaskDone,
  } = props;

  const [highlightTaskId, setHighlightTaskId] = useState<string | null>(null);

  const persona = filterPersona?.trim() ?? "";
  const visibleTasks = useMemo(() => {
    let list = coTasks;
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
  }, [coTasks, persona, dashboardFilter]);

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
            {persona ? `Tasks for ${persona}` : "All tasks"}
          </h2>
          <div className="flex flex-wrap gap-2">
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
          {persona
            ? "Work this role owns or has checked out. Use Assign / Hand back below to change who is active on a task."
            : "Full task list with checkout, SLA, and assignment—aligned with Command metrics and charts."}
        </p>
      </div>
      <ul className="divide-y divide-[#30363D]">
        {visibleTasks.map((t) => (
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
                  <StatusChip label={taskChipLabel(t)} tone={taskChipTone(t)} />
                  {t.owner_persona ? <span>Owner: {t.owner_persona}</span> : null}
                  {t.checked_out_by ? <span>With: {t.checked_out_by}</span> : null}
                  {t.checked_out_until ? <span>Until {String(t.checked_out_until)}</span> : null}
                  {t.due_at ? <span>Due {String(t.due_at)}</span> : null}
                  {t.sla_policy ? <span>Policy: {t.sla_policy}</span> : null}
                </div>
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
              </div>
            </div>
            {t.specification && (
              <pre className="mt-1 whitespace-pre-wrap font-mono text-[11px] text-gray-500">
                {t.specification}
              </pre>
            )}
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
        ))}
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

