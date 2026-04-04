"use client";

import { Dispatch, SetStateAction } from "react";
import {
  friendlyPolicyDecision,
  friendlyRisk,
  friendlyTaskState,
  queueTabMeta,
  type QueueView,
} from "../lib/inboxPlainLanguage";

function riskOptions(): { value: string; label: string }[] {
  return [
    { value: "low", label: friendlyRisk("low") },
    { value: "medium", label: friendlyRisk("medium") },
    { value: "high", label: friendlyRisk("high") },
    { value: "critical", label: friendlyRisk("critical") },
  ];
}
import { Panel } from "./Panel";
import { StatusChip, type ChipTone } from "./StatusChip";

export type { QueueView } from "../lib/inboxPlainLanguage";

type PolicyRule = {
  id: string;
  action_type: string;
  risk_level: string;
  amount_min?: number | null;
  amount_max?: number | null;
  decision_mode: string;
};

type TaskRow = {
  id: string;
  title: string;
  state: string;
  due_at?: string | null;
  priority?: number;
  decision_mode?: string;
};

type Props = {
  api: string;
  coSel: string;
  coPolicyAction: string;
  setCoPolicyAction: Dispatch<SetStateAction<string>>;
  coPolicyRisk: string;
  setCoPolicyRisk: Dispatch<SetStateAction<string>>;
  coPolicyAmtMin: string;
  setCoPolicyAmtMin: Dispatch<SetStateAction<string>>;
  coPolicyAmtMax: string;
  setCoPolicyAmtMax: Dispatch<SetStateAction<string>>;
  coPolicyDecision: string;
  setCoPolicyDecision: Dispatch<SetStateAction<string>>;
  coEvalAmount: string;
  setCoEvalAmount: Dispatch<SetStateAction<string>>;
  coPolicyEvalRes: string | null;
  setCoPolicyEvalRes: Dispatch<SetStateAction<string | null>>;
  coPolicyRules: PolicyRule[];
  coQueueView: QueueView;
  setCoQueueView: Dispatch<SetStateAction<QueueView>>;
  coQueueTasks: TaskRow[];
  coDecisionReason: Record<string, string>;
  setCoDecisionReason: Dispatch<SetStateAction<Record<string, string>>>;
  coCheckoutAgent: string;
  setCoErr: Dispatch<SetStateAction<string | null>>;
  loadCompanyOs: () => Promise<void>;
  loadQueueView: (viewOverride?: QueueView) => Promise<void>;
};

export function PolicyQueuePanel(props: Props) {
  const {
    api,
    coSel,
    coPolicyAction,
    setCoPolicyAction,
    coPolicyRisk,
    setCoPolicyRisk,
    coPolicyAmtMin,
    setCoPolicyAmtMin,
    coPolicyAmtMax,
    setCoPolicyAmtMax,
    coPolicyDecision,
    setCoPolicyDecision,
    coEvalAmount,
    setCoEvalAmount,
    coPolicyEvalRes,
    setCoPolicyEvalRes,
    coPolicyRules,
    coQueueView,
    setCoQueueView,
    coQueueTasks,
    coDecisionReason,
    setCoDecisionReason,
    coCheckoutAgent,
    setCoErr,
    loadCompanyOs,
    loadQueueView,
  } = props;

  const queueTabs: QueueView[] = ["all", "overdue", "atrisk", "waiting_admin", "pending_approvals", "blocked"];

  function queueItemTone(t: TaskRow, rawDecision: string): ChipTone {
    const dm = rawDecision.toLowerCase();
    if (t.state === "blocked" || dm === "blocked") return "red";
    if (t.state === "waiting_admin" || dm === "admin_required") return "amber";
    if (dm === "auto" || dm === "") return "green";
    return "gray";
  }

  return (
    <div className="mb-6 space-y-4">
      <Panel title="Queue" variant="console">
        <p className="mb-3 text-sm leading-relaxed text-[#8B949E]">
          Filters for what needs a human—same triage pattern as a dense ops console.
        </p>
        <div className="mb-2 flex flex-wrap gap-2">
          {queueTabs.map((v) => {
            const { label, hint } = queueTabMeta(v);
            return (
              <button
                key={v}
                type="button"
                title={hint}
                className={`rounded-md border px-3 py-1.5 font-mono text-[11px] font-medium uppercase tracking-wide transition-colors ${
                  coQueueView === v
                    ? "border-[#58a6ff]/50 bg-[#388bfd]/15 text-[#58a6ff]"
                    : "border-[#30363D] text-[#8B949E] hover:border-[#484f58] hover:text-[#c9d1d9]"
                }`}
                onClick={async () => {
                  setCoQueueView(v);
                  try {
                    await loadQueueView(v);
                  } catch (e) {
                    setCoErr(e instanceof Error ? e.message : String(e));
                  }
                }}
              >
                {label}
              </button>
            );
          })}
        </div>
        <ul className="max-h-[min(40vh,280px)] space-y-2 overflow-auto text-xs text-[#8B949E]">
          {coQueueTasks.map((t) => (
            <li key={t.id} className="rounded-lg border border-[#30363D] bg-[#010409] px-3 py-2">
              <div className="mb-1 flex items-center justify-between gap-2">
                <span className="text-[#c9d1d9]">
                  <span className="font-medium text-white">{t.title}</span>
                  {" · "}
                  {friendlyTaskState(t.state)}
                  {t.priority !== undefined ? ` · priority ${t.priority}` : ""}
                  {t.due_at ? ` · due ${t.due_at}` : ""}
                </span>
                <StatusChip
                  label={friendlyPolicyDecision(
                    t.decision_mode ?? (t.state === "waiting_admin" ? "admin_required" : t.state === "blocked" ? "blocked" : "auto")
                  )}
                  tone={queueItemTone(
                    t,
                    t.decision_mode ?? (t.state === "waiting_admin" ? "admin_required" : t.state === "blocked" ? "blocked" : "auto")
                  )}
                />
              </div>
              {(coQueueView === "pending_approvals" || t.state === "waiting_admin") && (
                <div className="flex items-center gap-2">
                  <input
                    className="min-w-0 flex-1 rounded border border-line bg-ink px-2 py-1 text-[11px]"
                    placeholder="Note for the record (optional)"
                    value={coDecisionReason[t.id] ?? ""}
                    onChange={(e) => setCoDecisionReason((m) => ({ ...m, [t.id]: e.target.value }))}
                  />
                  <button
                    type="button"
                    title="Let this task continue automatically"
                    className="rounded border border-emerald-700 px-2 py-1 text-[11px] text-emerald-300"
                    onClick={async () => {
                      setCoErr(null);
                      try {
                        const r = await fetch(`${api}/api/company/tasks/${t.id}/decision`, {
                          method: "POST",
                          headers: { "Content-Type": "application/json" },
                          body: JSON.stringify({
                            decision_mode: "auto",
                            actor: coCheckoutAgent.trim() || "admin",
                            reason: coDecisionReason[t.id] ?? "",
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
                    Allow
                  </button>
                  <button
                    type="button"
                    title="Stop this from proceeding automatically"
                    className="rounded border border-red-800 px-2 py-1 text-[11px] text-red-300"
                    onClick={async () => {
                      setCoErr(null);
                      try {
                        const r = await fetch(`${api}/api/company/tasks/${t.id}/decision`, {
                          method: "POST",
                          headers: { "Content-Type": "application/json" },
                          body: JSON.stringify({
                            decision_mode: "blocked",
                            actor: coCheckoutAgent.trim() || "admin",
                            reason: coDecisionReason[t.id] ?? "",
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
                    Don&apos;t allow
                  </button>
                </div>
              )}
            </li>
          ))}
          {!coQueueTasks.length && (
            <li className="text-[#484f58]">Nothing in this view—try another tab or add a task above.</li>
          )}
        </ul>
      </Panel>

      <details className="rounded-lg border border-line bg-panel">
        <summary className="cursor-pointer list-none px-4 py-3 text-sm font-medium text-gray-200 marker:content-none [&::-webkit-details-marker]:hidden">
          <span className="text-gray-400">▸</span> Automation rules{" "}
          <span className="font-normal text-gray-500">(optional — for people tuning AI behavior)</span>
        </summary>
        <div className="border-t border-line px-4 py-4">
        <p className="mb-3 text-sm leading-relaxed text-gray-400">
          Tell the system what AI may do alone, what must wait for you, and what is never automatic.
        </p>
        <div className="mb-2 grid max-w-3xl grid-cols-2 gap-2 sm:grid-cols-3">
          <input
            className="rounded border border-line bg-ink px-2 py-1 text-sm"
            placeholder="Type of action (e.g. send_message)"
            value={coPolicyAction}
            onChange={(e) => setCoPolicyAction(e.target.value)}
          />
          <select
            className="rounded border border-line bg-panel px-2 py-1 text-sm text-gray-200"
            value={coPolicyRisk}
            onChange={(e) => setCoPolicyRisk(e.target.value)}
          >
            {riskOptions().map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
          <input
            className="rounded border border-line bg-ink px-2 py-1 text-sm"
            placeholder="Min $ (optional)"
            value={coPolicyAmtMin}
            onChange={(e) => setCoPolicyAmtMin(e.target.value)}
          />
          <input
            className="rounded border border-line bg-ink px-2 py-1 text-sm"
            placeholder="Max $ (optional)"
            value={coPolicyAmtMax}
            onChange={(e) => setCoPolicyAmtMax(e.target.value)}
          />
          <select
            className="rounded border border-line bg-panel px-2 py-1 text-sm text-gray-200"
            value={coPolicyDecision}
            onChange={(e) => setCoPolicyDecision(e.target.value)}
          >
            <option value="auto">Runs on its own</option>
            <option value="admin_required">Ask me first</option>
            <option value="blocked">Never automatic</option>
          </select>
          <button
            type="button"
            className="rounded bg-accent/20 px-3 py-1 text-sm text-accent"
            onClick={async () => {
              setCoErr(null);
              try {
                const numOrUndef = (s: string) => {
                  const t = s.trim();
                  if (!t) return undefined;
                  const n = Number(t);
                  return Number.isFinite(n) ? n : undefined;
                };
                const r = await fetch(`${api}/api/company/companies/${coSel}/policies/rules`, {
                  method: "POST",
                  headers: { "Content-Type": "application/json" },
                  body: JSON.stringify({
                    action_type: coPolicyAction.trim(),
                    risk_level: coPolicyRisk.trim(),
                    amount_min: numOrUndef(coPolicyAmtMin),
                    amount_max: numOrUndef(coPolicyAmtMax),
                    decision_mode: coPolicyDecision.trim(),
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
            Add rule
          </button>
        </div>
        <div className="mb-2 flex flex-wrap gap-2">
          <input
            className="rounded border border-line bg-ink px-2 py-1 text-sm"
            placeholder="Test with a dollar amount (optional)"
            value={coEvalAmount}
            onChange={(e) => setCoEvalAmount(e.target.value)}
          />
          <button
            type="button"
            className="rounded border border-line px-3 py-1 text-sm text-gray-300"
            onClick={async () => {
              setCoErr(null);
              setCoPolicyEvalRes(null);
              try {
                const t = coEvalAmount.trim();
                const amount = t ? Number(t) : undefined;
                const r = await fetch(`${api}/api/company/companies/${coSel}/policies/evaluate`, {
                  method: "POST",
                  headers: { "Content-Type": "application/json" },
                  body: JSON.stringify({
                    action_type: coPolicyAction.trim(),
                    risk_level: coPolicyRisk.trim(),
                    amount: Number.isFinite(amount as number) ? amount : undefined,
                  }),
                });
                const j = await r.json();
                if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                setCoPolicyEvalRes(JSON.stringify(j, null, 2));
              } catch (e) {
                setCoErr(e instanceof Error ? e.message : String(e));
              }
            }}
          >
            Try it
          </button>
        </div>
        {coPolicyEvalRes && (
          <pre className="mb-2 max-h-[180px] overflow-auto rounded border border-line bg-ink p-2 font-mono text-[11px] text-gray-400">
            {coPolicyEvalRes}
          </pre>
        )}
        <ul className="max-h-[180px] space-y-1 overflow-auto text-xs text-gray-400">
          {coPolicyRules.map((r) => (
            <li key={r.id} className="rounded border border-line bg-ink/50 px-2 py-1 text-gray-300">
              <span className="font-medium text-gray-200">{r.action_type}</span>
              {" · "}
              {friendlyRisk(r.risk_level)} · {friendlyPolicyDecision(r.decision_mode)}
              {(r.amount_min ?? r.amount_max) !== undefined
                ? ` · if between $${r.amount_min ?? "—"} and $${r.amount_max ?? "—"}`
                : ""}
            </li>
          ))}
          {!coPolicyRules.length && <li className="text-gray-600">No rules yet—everything uses defaults.</li>}
        </ul>
        </div>
      </details>
    </div>
  );
}

