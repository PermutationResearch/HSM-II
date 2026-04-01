"use client";

import { Dispatch, SetStateAction } from "react";
import { Panel } from "./Panel";
import { StatusChip } from "./StatusChip";

export type QueueView = "all" | "overdue" | "atrisk" | "waiting_admin" | "pending_approvals" | "blocked";

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

  return (
    <div className="mb-4 grid gap-4 md:grid-cols-2">
      <Panel title="Policy rules">
        <div className="mb-2 grid grid-cols-2 gap-2">
          <input
            className="rounded border border-line bg-ink px-2 py-1 text-sm"
            placeholder="action_type"
            value={coPolicyAction}
            onChange={(e) => setCoPolicyAction(e.target.value)}
          />
          <select
            className="rounded border border-line bg-panel px-2 py-1 text-sm text-gray-200"
            value={coPolicyRisk}
            onChange={(e) => setCoPolicyRisk(e.target.value)}
          >
            <option value="low">low</option>
            <option value="medium">medium</option>
            <option value="high">high</option>
            <option value="critical">critical</option>
          </select>
          <input
            className="rounded border border-line bg-ink px-2 py-1 text-sm"
            placeholder="amount_min (optional)"
            value={coPolicyAmtMin}
            onChange={(e) => setCoPolicyAmtMin(e.target.value)}
          />
          <input
            className="rounded border border-line bg-ink px-2 py-1 text-sm"
            placeholder="amount_max (optional)"
            value={coPolicyAmtMax}
            onChange={(e) => setCoPolicyAmtMax(e.target.value)}
          />
          <select
            className="rounded border border-line bg-panel px-2 py-1 text-sm text-gray-200"
            value={coPolicyDecision}
            onChange={(e) => setCoPolicyDecision(e.target.value)}
          >
            <option value="auto">auto</option>
            <option value="admin_required">admin_required</option>
            <option value="blocked">blocked</option>
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
            placeholder="evaluate amount (optional)"
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
            Evaluate
          </button>
        </div>
        {coPolicyEvalRes && (
          <pre className="mb-2 max-h-[180px] overflow-auto rounded border border-line bg-ink p-2 font-mono text-[11px] text-gray-400">
            {coPolicyEvalRes}
          </pre>
        )}
        <ul className="max-h-[180px] space-y-1 overflow-auto text-xs text-gray-400">
          {coPolicyRules.map((r) => (
            <li key={r.id} className="rounded border border-line bg-ink/50 px-2 py-1">
              {r.action_type} · {r.risk_level} · {r.decision_mode}
              {(r.amount_min ?? r.amount_max) !== undefined
                ? ` · [${r.amount_min ?? "-inf"} .. ${r.amount_max ?? "+inf"}]`
                : ""}
            </li>
          ))}
          {!coPolicyRules.length && <li className="text-gray-600">No policy rules yet.</li>}
        </ul>
      </Panel>

      <Panel title="Queue views">
        <div className="mb-2 flex flex-wrap gap-2">
          {(["all", "overdue", "atrisk", "pending_approvals", "blocked"] as QueueView[]).map((v) => (
            <button
              key={v}
              type="button"
              className={`rounded border px-2 py-1 text-xs ${
                coQueueView === v ? "border-accent bg-accent/10 text-accent" : "border-line text-gray-400"
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
              {v}
            </button>
          ))}
        </div>
        <ul className="max-h-[220px] space-y-1 overflow-auto text-xs text-gray-400">
          {coQueueTasks.map((t) => (
            <li key={t.id} className="rounded border border-line bg-ink/50 px-2 py-1">
              <div className="mb-1 flex items-center justify-between gap-2">
                <span>
                  {t.title} · {t.state}
                  {t.priority !== undefined ? ` · p${t.priority}` : ""}
                  {t.due_at ? ` · due ${t.due_at}` : ""}
                </span>
                <StatusChip
                  label={(t.decision_mode ?? (t.state === "waiting_admin" ? "admin_required" : t.state === "blocked" ? "blocked" : "auto")).toUpperCase()}
                />
              </div>
              {(coQueueView === "pending_approvals" || t.state === "waiting_admin") && (
                <div className="flex items-center gap-2">
                  <input
                    className="min-w-0 flex-1 rounded border border-line bg-ink px-2 py-1 text-[11px]"
                    placeholder="reason (optional)"
                    value={coDecisionReason[t.id] ?? ""}
                    onChange={(e) => setCoDecisionReason((m) => ({ ...m, [t.id]: e.target.value }))}
                  />
                  <button
                    type="button"
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
                    Approve
                  </button>
                  <button
                    type="button"
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
                    Block
                  </button>
                </div>
              )}
            </li>
          ))}
          {!coQueueTasks.length && <li className="text-gray-600">No queue tasks in this view.</li>}
        </ul>
      </Panel>
    </div>
  );
}

