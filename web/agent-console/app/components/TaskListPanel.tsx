"use client";

import { Dispatch, SetStateAction } from "react";
import { StatusChip } from "./StatusChip";

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
};

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
  } = props;

  return (
    <div className="rounded border border-line bg-panel">
      <div className="border-b border-line px-3 py-2 text-xs uppercase text-gray-500">Tasks</div>
      <ul className="divide-y divide-line">
        {coTasks.map((t) => (
          <li key={t.id} className="px-3 py-2 text-sm">
            <div className="flex flex-wrap items-start justify-between gap-2">
              <div>
                <div className="font-medium text-gray-200">{t.title}</div>
                <div className="text-xs text-gray-500">
                  {t.state}
                  {" · "}
                  <StatusChip
                    label={t.state === "blocked" ? "BLOCKED" : t.state === "waiting_admin" ? "ADMIN_REQUIRED" : "AUTO"}
                  />
                  {t.owner_persona ? ` · ${t.owner_persona}` : ""}
                  {t.checked_out_by ? ` · out: ${t.checked_out_by}` : ""}
                  {t.checked_out_until ? ` · until ${String(t.checked_out_until)}` : ""}
                  {t.due_at ? ` · due ${String(t.due_at)}` : ""}
                  {t.sla_policy ? ` · SLA ${t.sla_policy}` : ""}
                </div>
              </div>
              <div className="flex shrink-0 gap-2">
                <button
                  type="button"
                  className="rounded border border-accent/40 bg-accent/10 px-2 py-1 text-xs text-accent hover:bg-accent/20"
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
                  Check out
                </button>
                <button
                  type="button"
                  className="rounded border border-line px-2 py-1 text-xs text-gray-400 hover:bg-white/5"
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
                  Release
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
            <div className="mt-2 grid grid-cols-1 gap-2 md:grid-cols-5">
              <input
                className="rounded border border-line bg-ink px-2 py-1 text-xs"
                placeholder="due_at (ISO)"
                value={coSlaDueAt[t.id] ?? ""}
                onChange={(e) => setCoSlaDueAt((m) => ({ ...m, [t.id]: e.target.value }))}
              />
              <input
                className="rounded border border-line bg-ink px-2 py-1 text-xs"
                placeholder="escalate_after (ISO)"
                value={coSlaEscAt[t.id] ?? ""}
                onChange={(e) => setCoSlaEscAt((m) => ({ ...m, [t.id]: e.target.value }))}
              />
              <input
                className="rounded border border-line bg-ink px-2 py-1 text-xs"
                placeholder="sla_policy"
                value={coSlaPol[t.id] ?? ""}
                onChange={(e) => setCoSlaPol((m) => ({ ...m, [t.id]: e.target.value }))}
              />
              <input
                className="rounded border border-line bg-ink px-2 py-1 text-xs"
                placeholder="priority"
                value={coSlaPrio[t.id] ?? ""}
                onChange={(e) => setCoSlaPrio((m) => ({ ...m, [t.id]: e.target.value }))}
              />
              <div className="flex gap-2">
                <input
                  className="min-w-0 flex-1 rounded border border-line bg-ink px-2 py-1 text-xs"
                  placeholder="status_reason"
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
                  Save SLA
                </button>
              </div>
            </div>
          </li>
        ))}
        {!coTasks.length && <li className="px-3 py-4 text-gray-600">No tasks.</li>}
      </ul>
    </div>
  );
}

