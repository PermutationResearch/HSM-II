"use client";

import { CheckCircle2, Inbox, RefreshCw, XCircle } from "lucide-react";
import { Dispatch, SetStateAction, useState } from "react";
import {
  friendlyPolicyDecision,
  friendlyRisk,
  friendlyTaskState,
  queueTabMeta,
  type QueueView,
} from "../lib/inboxPlainLanguage";

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
  requires_human?: boolean;
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

/** What kind of action does this item need? */
function itemKind(t: TaskRow): "approval" | "blocked" | "attention" {
  if (t.state === "waiting_admin" || t.decision_mode === "admin_required") return "approval";
  if (t.state === "blocked" || t.decision_mode === "blocked") return "blocked";
  return "attention";
}

function ItemBadge({ kind }: { kind: ReturnType<typeof itemKind> }) {
  if (kind === "approval")
    return (
      <span className="rounded-full bg-amber-500/15 px-2 py-0.5 font-mono text-[10px] font-semibold uppercase tracking-wide text-amber-400">
        Needs approval
      </span>
    );
  if (kind === "blocked")
    return (
      <span className="rounded-full bg-red-500/15 px-2 py-0.5 font-mono text-[10px] font-semibold uppercase tracking-wide text-red-400">
        Blocked
      </span>
    );
  return (
    <span className="rounded-full bg-blue-500/15 px-2 py-0.5 font-mono text-[10px] font-semibold uppercase tracking-wide text-blue-400">
      Agent asked for you
    </span>
  );
}

function leftBorderColor(kind: ReturnType<typeof itemKind>) {
  if (kind === "approval") return "border-l-amber-500";
  if (kind === "blocked") return "border-l-red-500";
  return "border-l-blue-500";
}

const OTHER_VIEWS: QueueView[] = ["all", "overdue", "atrisk", "waiting_admin", "pending_approvals", "blocked"];

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

  const [refreshing, setRefreshing] = useState(false);
  const [showFilters, setShowFilters] = useState(false);
  const [showRules, setShowRules] = useState(false);

  const isInbox = coQueueView === "human_inbox";
  const count = coQueueTasks.length;

  const refresh = async () => {
    setRefreshing(true);
    try {
      await loadCompanyOs();
      await loadQueueView();
    } finally {
      setRefreshing(false);
    }
  };

  const switchView = async (v: QueueView) => {
    setCoQueueView(v);
    try {
      await loadQueueView(v);
    } catch (e) {
      setCoErr(e instanceof Error ? e.message : String(e));
    }
  };

  const actor = coCheckoutAgent.trim() || "admin";

  const doDecision = async (taskId: string, mode: string, endpoint = "decision") => {
    setCoErr(null);
    try {
      const body =
        endpoint === "requires-human"
          ? { requires_human: false, actor, reason: coDecisionReason[taskId] ?? "" }
          : { decision_mode: mode, actor, reason: coDecisionReason[taskId] ?? "" };

      const r = await fetch(`${api}/api/company/tasks/${taskId}/${endpoint}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      const j = await r.json();
      if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
      await refresh();
    } catch (e) {
      setCoErr(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="space-y-4">
      {/* ── Header ── */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2.5">
          <Inbox className="h-5 w-5 text-[#8B949E]" strokeWidth={1.5} />
          <h2 className="text-base font-semibold text-[#E6EDF3]">
            {isInbox ? "Inbox" : queueTabMeta(coQueueView).label}
          </h2>
          {count > 0 && (
            <span className="rounded-full bg-[#388bfd]/20 px-2 py-0.5 font-mono text-xs font-semibold text-[#58a6ff]">
              {count}
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            title="Other views"
            onClick={() => setShowFilters((s) => !s)}
            className="rounded-md border border-[#30363D] px-2.5 py-1 font-mono text-[11px] text-[#8B949E] transition-colors hover:border-[#484f58] hover:text-[#c9d1d9]"
          >
            {showFilters ? "Hide filters" : "Filters"}
          </button>
          <button
            type="button"
            title="Refresh inbox"
            onClick={refresh}
            className="flex h-7 w-7 items-center justify-center rounded-md border border-[#30363D] text-[#8B949E] transition-colors hover:border-[#484f58] hover:text-[#c9d1d9]"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${refreshing ? "animate-spin" : ""}`} strokeWidth={1.5} />
          </button>
        </div>
      </div>

      {/* ── Filter pills (collapsed by default) ── */}
      {showFilters && (
        <div className="flex flex-wrap gap-1.5">
          <button
            type="button"
            onClick={() => switchView("human_inbox")}
            className={`rounded-full border px-3 py-1 font-mono text-[11px] font-medium transition-colors ${
              coQueueView === "human_inbox"
                ? "border-[#58a6ff]/50 bg-[#388bfd]/15 text-[#58a6ff]"
                : "border-[#30363D] text-[#8B949E] hover:border-[#484f58] hover:text-[#c9d1d9]"
            }`}
          >
            Inbox
          </button>
          {OTHER_VIEWS.map((v) => (
            <button
              key={v}
              type="button"
              title={queueTabMeta(v).hint}
              onClick={() => switchView(v)}
              className={`rounded-full border px-3 py-1 font-mono text-[11px] font-medium transition-colors ${
                coQueueView === v
                  ? "border-[#58a6ff]/50 bg-[#388bfd]/15 text-[#58a6ff]"
                  : "border-[#30363D] text-[#8B949E] hover:border-[#484f58] hover:text-[#c9d1d9]"
              }`}
            >
              {queueTabMeta(v).label}
            </button>
          ))}
        </div>
      )}

      {/* ── Item feed ── */}
      {coQueueTasks.length === 0 ? (
        <div className="flex flex-col items-center gap-3 rounded-xl border border-[#21262D] bg-[#0D1117] py-14 text-center">
          <CheckCircle2 className="h-9 w-9 text-emerald-500/60" strokeWidth={1.5} />
          <p className="text-sm font-medium text-[#8B949E]">
            {isInbox ? "All clear — no agents waiting on you" : "Nothing in this view"}
          </p>
          {isInbox && (
            <p className="max-w-xs text-xs text-[#484f58]">
              Agents will surface tasks here when they need a decision, approval, or are stuck.
            </p>
          )}
        </div>
      ) : (
        <ul className="space-y-3">
          {coQueueTasks.map((t) => {
            const kind = itemKind(t);
            const borderColor = leftBorderColor(kind);
            const needsDecision =
              kind === "approval" || kind === "blocked";
            const needsAck = kind === "attention";

            return (
              <li
                key={t.id}
                className={`rounded-xl border border-[#21262D] border-l-2 bg-[#0D1117] ${borderColor}`}
              >
                {/* Card header */}
                <div className="flex items-start justify-between gap-3 px-4 pt-4 pb-2">
                  <div className="min-w-0 flex-1">
                    <div className="mb-1.5">
                      <ItemBadge kind={kind} />
                    </div>
                    <p className="text-sm font-medium leading-snug text-[#E6EDF3]">{t.title}</p>
                    <div className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 font-mono text-[11px] text-[#484f58]">
                      <span>{friendlyTaskState(t.state)}</span>
                      {t.decision_mode && t.decision_mode !== "auto" && (
                        <>
                          <span>·</span>
                          <span>{friendlyPolicyDecision(t.decision_mode)}</span>
                        </>
                      )}
                      {t.priority !== undefined && (
                        <>
                          <span>·</span>
                          <span>priority {t.priority}</span>
                        </>
                      )}
                      {t.due_at && (
                        <>
                          <span>·</span>
                          <span className="text-amber-500/80">due {new Date(t.due_at).toLocaleDateString()}</span>
                        </>
                      )}
                    </div>
                  </div>
                </div>

                {/* Action row */}
                <div className="flex flex-wrap items-center gap-2 border-t border-[#21262D] px-4 py-3">
                  <input
                    className="min-w-0 flex-1 rounded-lg border border-[#30363D] bg-[#161b22] px-3 py-1.5 text-xs text-[#E6EDF3] outline-none placeholder:text-[#6E7681] focus:border-[#58a6ff]"
                    placeholder={needsDecision ? "Note for the record (optional)" : "Reply to agent (optional, logged)"}
                    value={coDecisionReason[t.id] ?? ""}
                    onChange={(e) => setCoDecisionReason((m) => ({ ...m, [t.id]: e.target.value }))}
                  />

                  {needsDecision && (
                    <>
                      <button
                        type="button"
                        title="Reject — keep blocked"
                        onClick={() => doDecision(t.id, "blocked")}
                        className="flex items-center gap-1.5 rounded-lg bg-[#dc2626] px-3 py-1.5 text-xs font-semibold text-white transition-colors hover:bg-[#b91c1c]"
                      >
                        <XCircle className="h-3.5 w-3.5" strokeWidth={1.5} />
                        Reject
                      </button>
                      <button
                        type="button"
                        title="Approve — unblock the agent"
                        onClick={() => doDecision(t.id, "auto")}
                        className="flex items-center gap-1.5 rounded-lg bg-[#10b981] px-3 py-1.5 text-xs font-semibold text-white transition-colors hover:bg-[#059669]"
                      >
                        <CheckCircle2 className="h-3.5 w-3.5" strokeWidth={1.5} />
                        Approve
                      </button>
                    </>
                  )}

                  {needsAck && (
                    <>
                      <button
                        type="button"
                        title="Dismiss without a note"
                        onClick={() => doDecision(t.id, "", "requires-human")}
                        className="rounded-lg border border-[#30363D] px-3 py-1.5 text-xs font-medium text-[#8B949E] transition-colors hover:border-[#484f58] hover:text-[#c9d1d9]"
                      >
                        Dismiss
                      </button>
                      <button
                        type="button"
                        title="Send reply and let agent resume"
                        onClick={() => doDecision(t.id, "", "requires-human")}
                        className="flex items-center gap-1.5 rounded-lg bg-[#10b981] px-3 py-1.5 text-xs font-semibold text-white transition-colors hover:bg-[#059669]"
                      >
                        <CheckCircle2 className="h-3.5 w-3.5" strokeWidth={1.5} />
                        Resume agent
                      </button>
                    </>
                  )}
                </div>
              </li>
            );
          })}
        </ul>
      )}

      {/* ── Automation rules (collapsed) ── */}
      <details
        open={showRules}
        onToggle={(e) => setShowRules((e.target as HTMLDetailsElement).open)}
        className="rounded-xl border border-[#21262D] bg-[#0D1117]"
      >
        <summary className="flex cursor-pointer list-none items-center justify-between px-4 py-3 text-sm font-medium text-[#8B949E] transition-colors hover:text-[#c9d1d9] [&::-webkit-details-marker]:hidden">
          <span>Automation rules</span>
          <span className="font-mono text-[11px] text-[#484f58]">
            {coPolicyRules.length > 0 ? `${coPolicyRules.length} rules` : "none yet"}
          </span>
        </summary>
        <div className="border-t border-[#21262D] px-4 py-4 space-y-4">
          <p className="text-xs leading-relaxed text-[#8B949E]">
            Tell the system what agents may do alone, what must wait for you, and what is never automatic.
          </p>

          {/* Rule builder */}
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
            <input
              className="rounded-lg border border-[#30363D] bg-[#010409] px-2.5 py-1.5 text-xs text-[#c9d1d9] placeholder:text-[#484f58] outline-none focus:border-[#58a6ff]"
              placeholder="Action type (e.g. send_message)"
              value={coPolicyAction}
              onChange={(e) => setCoPolicyAction(e.target.value)}
            />
            <select
              className="rounded-lg border border-[#30363D] bg-[#0D1117] px-2.5 py-1.5 text-xs text-[#c9d1d9] outline-none focus:border-[#58a6ff]"
              value={coPolicyRisk}
              onChange={(e) => setCoPolicyRisk(e.target.value)}
            >
              {(["low", "medium", "high", "critical"] as const).map((v) => (
                <option key={v} value={v}>{friendlyRisk(v)}</option>
              ))}
            </select>
            <input
              className="rounded-lg border border-[#30363D] bg-[#010409] px-2.5 py-1.5 text-xs text-[#c9d1d9] placeholder:text-[#484f58] outline-none focus:border-[#58a6ff]"
              placeholder="Min $ (optional)"
              value={coPolicyAmtMin}
              onChange={(e) => setCoPolicyAmtMin(e.target.value)}
            />
            <input
              className="rounded-lg border border-[#30363D] bg-[#010409] px-2.5 py-1.5 text-xs text-[#c9d1d9] placeholder:text-[#484f58] outline-none focus:border-[#58a6ff]"
              placeholder="Max $ (optional)"
              value={coPolicyAmtMax}
              onChange={(e) => setCoPolicyAmtMax(e.target.value)}
            />
            <select
              className="rounded-lg border border-[#30363D] bg-[#0D1117] px-2.5 py-1.5 text-xs text-[#c9d1d9] outline-none focus:border-[#58a6ff]"
              value={coPolicyDecision}
              onChange={(e) => setCoPolicyDecision(e.target.value)}
            >
              <option value="auto">Runs on its own</option>
              <option value="admin_required">Ask me first</option>
              <option value="blocked">Never automatic</option>
            </select>
            <button
              type="button"
              className="rounded-lg bg-[#388bfd] px-3 py-1.5 text-xs font-semibold text-white transition-colors hover:bg-[#4493ff]"
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

          {/* Test evaluator */}
          <div className="flex flex-wrap items-center gap-2">
            <input
              className="rounded-lg border border-[#30363D] bg-[#010409] px-2.5 py-1.5 text-xs text-[#c9d1d9] placeholder:text-[#484f58] outline-none focus:border-[#58a6ff]"
              placeholder="Test with a dollar amount"
              value={coEvalAmount}
              onChange={(e) => setCoEvalAmount(e.target.value)}
            />
            <button
              type="button"
              className="rounded-lg border border-[#30363D] px-3 py-1.5 text-xs text-[#8B949E] transition-colors hover:border-[#484f58] hover:text-[#c9d1d9]"
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
            <pre className="max-h-[180px] overflow-auto rounded-lg border border-[#21262D] bg-[#010409] p-3 font-mono text-[11px] text-[#8B949E]">
              {coPolicyEvalRes}
            </pre>
          )}

          {/* Existing rules */}
          <ul className="max-h-[180px] space-y-1 overflow-auto">
            {coPolicyRules.map((r) => (
              <li key={r.id} className="rounded-lg border border-[#21262D] bg-[#010409] px-3 py-2 text-xs text-[#8B949E]">
                <span className="font-medium text-[#c9d1d9]">{r.action_type}</span>
                {" · "}
                {friendlyRisk(r.risk_level)} · {friendlyPolicyDecision(r.decision_mode)}
                {(r.amount_min ?? r.amount_max) !== undefined
                  ? ` · $${r.amount_min ?? "—"}–$${r.amount_max ?? "—"}`
                  : ""}
              </li>
            ))}
            {!coPolicyRules.length && (
              <li className="text-[#484f58]">No rules yet — everything uses defaults.</li>
            )}
          </ul>
        </div>
      </details>
    </div>
  );
}
