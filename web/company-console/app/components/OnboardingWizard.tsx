"use client";

import { Dispatch, SetStateAction } from "react";
import { useEffect, useMemo, useState } from "react";
import { ActionBar } from "./ActionBar";
import { ConfidenceBar } from "./ConfidenceBar";
import { EmptyState } from "./EmptyState";
import { Panel } from "./Panel";

export type OnboardWorkflow = {
  title: string;
  owner_role: string;
  priority: string;
  sla_target: string;
  approval: string;
};

export type OnboardPolicy = {
  action_type: string;
  risk_level: string;
  decision_mode: string;
  amount_min?: number | null;
  amount_max?: number | null;
  approver_role: string;
};

export type OnboardDraft = {
  company_name: string;
  industry: string;
  vertical_template: string;
  pack_contract_id: string;
  workflows: OnboardWorkflow[];
  policy_rules: OnboardPolicy[];
  kpi_gates?: OnboardGateResult[];
  risk_gates?: OnboardGateResult[];
  missing_critical_items: string[];
  confidence_by_field: Record<string, number>;
};

type OnboardGateResult = {
  id: string;
  label: string;
  required: boolean;
  satisfied: boolean;
  evidence_hint: string;
};

type PackContract = {
  id: string;
  vertical: string;
  display_name: string;
  description: string;
};

/** Example that satisfies `property_management_ops_v1` gates + server `missing_critical` heuristics. */
export const GESTION_VELORA_EXAMPLE_TRANSCRIPT = [
  "We are Gestion Velora Inc., residential property management in Montreal and Laval.",
  "Emergency maintenance same day. Standard work orders: we acknowledge the tenant within 24h and follow up until the work order is closed.",
  "Routine tenant email and portal messages get a written response within one business day.",
  "Legal notices, lease disputes, and fair housing complaints escalate to outside counsel—never auto-replied by staff.",
  "Refunds, rent credits, and operating budget changes require owner approval before we act.",
] as const;

const VELORA_COMPANY_NAME = "Gestion Velora Inc.";

type Props = {
  api: string;
  obVertical: string;
  setObVertical: Dispatch<SetStateAction<string>>;
  obInput: string;
  setObInput: Dispatch<SetStateAction<string>>;
  obTranscript: string[];
  setObTranscript: Dispatch<SetStateAction<string[]>>;
  obLoading: boolean;
  setObLoading: Dispatch<SetStateAction<boolean>>;
  obDraft: OnboardDraft | null;
  setObDraft: Dispatch<SetStateAction<OnboardDraft | null>>;
  obApplyLoading: boolean;
  setObApplyLoading: Dispatch<SetStateAction<boolean>>;
  obApplyMsg: string | null;
  setObApplyMsg: Dispatch<SetStateAction<string | null>>;
  obNextQuestion: string;
  setErr: Dispatch<SetStateAction<string | null>>;
  onApplySuccess: (companyId: string) => Promise<void>;
};

export function OnboardingWizard(props: Props) {
  const {
    api,
    obVertical,
    setObVertical,
    obInput,
    setObInput,
    obTranscript,
    setObTranscript,
    obLoading,
    setObLoading,
    obDraft,
    setObDraft,
    obApplyLoading,
    setObApplyLoading,
    obApplyMsg,
    setObApplyMsg,
    obNextQuestion,
    setErr,
    onApplySuccess,
  } = props;
  const [contracts, setContracts] = useState<PackContract[]>([]);
  const [selectedPack, setSelectedPack] = useState("");

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const r = await fetch(`${api}/api/company/onboarding/contracts`);
        const j = (await r.json()) as { contracts?: PackContract[] };
        if (!r.ok) return;
        if (!alive) return;
        const list = j.contracts ?? [];
        setContracts(list);
        if (!selectedPack) {
          const pref = list.find((c) => c.vertical === obVertical)?.id ?? list[0]?.id ?? "";
          setSelectedPack(pref);
        }
      } catch {
        // keep onboarding usable even if contract catalog fetch fails
      }
    })();
    return () => {
      alive = false;
    };
  }, [api, obVertical, selectedPack]);

  useEffect(() => {
    if (!contracts.length) return;
    const pref = contracts.find((c) => c.vertical === obVertical)?.id ?? contracts[0]?.id ?? "";
    setSelectedPack((cur) => cur || pref);
  }, [contracts, obVertical]);

  const unsatisfiedRequiredGates = useMemo(
    () =>
      [...(obDraft?.kpi_gates ?? []), ...(obDraft?.risk_gates ?? [])].filter(
        (g) => g.required && !g.satisfied
      ),
    [obDraft]
  );

  async function refreshDraft(
    transcript: string[],
    opts?: { companyName?: string; verticalTemplate?: string; packContractId?: string }
  ) {
    if (!transcript.length) return;
    setErr(null);
    setObLoading(true);
    try {
      const vertical = opts?.verticalTemplate ?? obVertical;
      const packId = opts?.packContractId ?? selectedPack;
      const r = await fetch(`${api}/api/company/onboarding/draft`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          transcript: transcript.join("\n"),
          vertical_template: vertical,
          pack_contract_id: packId,
          company_name: opts?.companyName ?? obDraft?.company_name ?? "",
        }),
      });
      const j = await r.json();
      if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
      setObDraft((j as { draft: OnboardDraft }).draft);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setObLoading(false);
    }
  }

  return (
    <>
      <h1 className="mb-2 text-lg font-medium text-white">Onboarding wizard</h1>
      <p className="mb-4 max-w-3xl text-sm text-gray-500">
        Chat-driven intake converts business language into draft workflows, policy rules, SLA defaults,
        and ownership. Review, quick-edit, then approve all.
      </p>
      {obApplyMsg && (
        <div className="mb-4 rounded border border-emerald-900/50 bg-emerald-950/30 px-3 py-2 text-sm text-emerald-200">
          {obApplyMsg}
        </div>
      )}
      <ActionBar>
        <select
          className="rounded border border-line bg-panel px-2 py-1 text-sm text-gray-200"
          value={obVertical}
          onChange={(e) => setObVertical(e.target.value)}
        >
          <option value="generic_smb">generic_smb</option>
          <option value="ecommerce">ecommerce</option>
          <option value="marketing">marketing</option>
          <option value="property_management">property_management</option>
        </select>
        <button
          type="button"
          className="rounded border border-line px-3 py-1 text-sm text-gray-300"
          onClick={() => void refreshDraft(obTranscript)}
        >
          Refresh draft
        </button>
        <button
          type="button"
          className="rounded border border-emerald-900/50 bg-emerald-950/30 px-3 py-1 text-sm text-emerald-200"
          disabled={obLoading}
          onClick={() => {
            const pmPack =
              contracts.find((c) => c.id === "property_management_ops_v1") ??
              contracts.find((c) => c.vertical === "property_management");
            const packId = pmPack?.id ?? "property_management_ops_v1";
            setObVertical("property_management");
            setSelectedPack(packId);
            const lines = [...GESTION_VELORA_EXAMPLE_TRANSCRIPT];
            setObTranscript(lines);
            setObInput("");
            setObApplyMsg(null);
            void refreshDraft(lines, {
              companyName: VELORA_COMPANY_NAME,
              verticalTemplate: "property_management",
              packContractId: packId,
            });
          }}
        >
          Example: Gestion Velora
        </button>
        <select
          className="rounded border border-line bg-panel px-2 py-1 text-sm text-gray-200"
          value={selectedPack}
          onChange={(e) => setSelectedPack(e.target.value)}
        >
          {contracts.map((c) => (
            <option key={c.id} value={c.id}>
              {c.display_name}
            </option>
          ))}
        </select>
      </ActionBar>
      <div className="mb-4 grid gap-4 md:grid-cols-2">
        <Panel title="Interview">
          <div className="mb-2 max-h-[220px] space-y-1 overflow-auto rounded border border-line bg-ink/40 p-2 text-xs text-gray-300">
            {obTranscript.map((m, i) => (
              <div key={i}>- {m}</div>
            ))}
            {!obTranscript.length && <EmptyState message="No answers yet." />}
          </div>
          <textarea
            className="mb-2 min-h-[80px] w-full rounded border border-line bg-ink px-2 py-1 text-sm"
            placeholder="Tell us how your business works day-to-day..."
            value={obInput}
            onChange={(e) => setObInput(e.target.value)}
          />
          <div className="flex gap-2">
            <button
              type="button"
              className="rounded bg-accent/20 px-3 py-1 text-sm text-accent"
              disabled={obLoading}
              onClick={async () => {
                const msg = obInput.trim();
                if (!msg) return;
                const next = [...obTranscript, msg];
                setObTranscript(next);
                setObInput("");
                await refreshDraft(next);
              }}
            >
              {obLoading ? "Thinking..." : "Add answer"}
            </button>
            <button
              type="button"
              className="rounded border border-line px-3 py-1 text-sm text-gray-400"
              onClick={() => {
                setObTranscript([]);
                setObInput("");
                setObDraft(null);
                setObApplyMsg(null);
              }}
            >
              Reset
            </button>
          </div>
        </Panel>
        <Panel title="Progressive summary">
          {!obDraft ? (
            <EmptyState message="Start the interview to generate a draft." />
          ) : (
            <>
              <input
                className="mb-2 w-full rounded border border-line bg-ink px-2 py-1 text-sm"
                placeholder="Company name"
                value={obDraft.company_name}
                onChange={(e) =>
                  setObDraft((d) => (d ? { ...d, company_name: e.target.value } : d))
                }
              />
              <div className="mb-2 text-xs text-gray-500">
                template: {obDraft.vertical_template} · industry: {obDraft.industry} · contract:{" "}
                {obDraft.pack_contract_id}
              </div>
              <div className="mb-2 text-xs text-gray-400">
                Missing critical:{" "}
                {obDraft.missing_critical_items.length
                  ? obDraft.missing_critical_items.join(", ")
                  : "none"}
              </div>
              <div className="mb-2 rounded border border-amber-900/50 bg-amber-950/20 px-2 py-1 text-xs text-amber-200">
                Next question: {obNextQuestion}
              </div>
              <div className="mb-2 rounded border border-line bg-ink/40 p-2">
                <div className="mb-2 text-xs uppercase text-gray-500">Confidence by field</div>
                <div className="space-y-2">
                  {Object.entries(obDraft.confidence_by_field ?? {}).map(([k, v]) => (
                    <ConfidenceBar key={k} label={k} value={v} />
                  ))}
                </div>
              </div>
              {!!(obDraft.kpi_gates?.length || obDraft.risk_gates?.length) && (
                <div className="mb-2 rounded border border-line bg-ink/40 p-2 text-xs">
                  <div className="mb-1 uppercase text-gray-500">KPI / risk gates</div>
                  <div className="space-y-1">
                    {[...(obDraft.kpi_gates ?? []), ...(obDraft.risk_gates ?? [])].map((g) => (
                      <div key={g.id} className={g.satisfied ? "text-emerald-300" : "text-amber-300"}>
                        {g.satisfied ? "OK" : "MISSING"} · {g.label}
                        {!g.satisfied ? ` (${g.evidence_hint})` : ""}
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </>
          )}
        </Panel>
      </div>
      {obDraft && (
        <div className="mb-4 rounded border border-line bg-panel p-3">
          <div className="mb-2 text-xs uppercase text-gray-500">Review + quick edits</div>
          <div className="mb-2 text-xs text-gray-400">Workflows</div>
          <div className="mb-3 space-y-2">
            {obDraft.workflows.map((w, i) => (
              <div key={i} className="grid gap-2 md:grid-cols-5">
                <input
                  className="rounded border border-line bg-ink px-2 py-1 text-xs"
                  value={w.title}
                  onChange={(e) =>
                    setObDraft((d) =>
                      d
                        ? {
                            ...d,
                            workflows: d.workflows.map((x, ix) =>
                              ix === i ? { ...x, title: e.target.value } : x
                            ),
                          }
                        : d
                    )
                  }
                />
                <input
                  className="rounded border border-line bg-ink px-2 py-1 text-xs"
                  value={w.owner_role}
                  onChange={(e) =>
                    setObDraft((d) =>
                      d
                        ? {
                            ...d,
                            workflows: d.workflows.map((x, ix) =>
                              ix === i ? { ...x, owner_role: e.target.value } : x
                            ),
                          }
                        : d
                    )
                  }
                />
                <input className="rounded border border-line bg-ink px-2 py-1 text-xs" value={w.priority} onChange={(e) => setObDraft((d) => d ? ({ ...d, workflows: d.workflows.map((x, ix) => ix === i ? { ...x, priority: e.target.value } : x) }) : d)} />
                <input className="rounded border border-line bg-ink px-2 py-1 text-xs" value={w.sla_target} onChange={(e) => setObDraft((d) => d ? ({ ...d, workflows: d.workflows.map((x, ix) => ix === i ? { ...x, sla_target: e.target.value } : x) }) : d)} />
                <input className="rounded border border-line bg-ink px-2 py-1 text-xs" value={w.approval} onChange={(e) => setObDraft((d) => d ? ({ ...d, workflows: d.workflows.map((x, ix) => ix === i ? { ...x, approval: e.target.value } : x) }) : d)} />
              </div>
            ))}
          </div>
          <div className="mb-2 text-xs text-gray-400">Policy rules</div>
          <div className="mb-3 max-h-[220px] space-y-1 overflow-auto text-xs text-gray-400">
            {obDraft.policy_rules.map((r, i) => (
              <div key={i} className="rounded border border-line bg-ink/40 px-2 py-1">
                {r.action_type} · {r.risk_level} · {r.decision_mode} · approver {r.approver_role}
              </div>
            ))}
          </div>
          <button
            type="button"
            className="rounded bg-accent/20 px-3 py-1 text-sm text-accent disabled:cursor-not-allowed disabled:opacity-50"
            disabled={
              obApplyLoading ||
              (obDraft.missing_critical_items?.length ?? 0) > 0 ||
              unsatisfiedRequiredGates.length > 0
            }
            onClick={async () => {
              if (!obDraft) return;
              setErr(null);
              setObApplyMsg(null);
              setObApplyLoading(true);
              try {
                const r = await fetch(`${api}/api/company/onboarding/apply`, {
                  method: "POST",
                  headers: { "Content-Type": "application/json" },
                  body: JSON.stringify({ draft: obDraft }),
                });
                const j = await r.json();
                if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                const cid = (j as { company_id?: string }).company_id;
                setObApplyMsg(`Applied onboarding draft${cid ? ` · company_id ${cid}` : ""}.`);
                if (cid) await onApplySuccess(cid);
              } catch (e) {
                setErr(e instanceof Error ? e.message : String(e));
              } finally {
                setObApplyLoading(false);
              }
            }}
          >
            {obApplyLoading ? "Applying..." : "Approve all"}
          </button>
          {!!obDraft.missing_critical_items.length && (
            <div className="mt-2 text-xs text-amber-300">
              Approve all is blocked until missing critical items are resolved:{" "}
              {obDraft.missing_critical_items.join(", ")}
            </div>
          )}
          {!!unsatisfiedRequiredGates.length && (
            <div className="mt-2 text-xs text-amber-300">
              Required KPI/risk gates missing: {unsatisfiedRequiredGates.map((g) => g.id).join(", ")}
            </div>
          )}
        </div>
      )}
    </>
  );
}

