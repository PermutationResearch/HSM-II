"use client";

import { useEffect, useMemo, useState } from "react";
import { Panel } from "./Panel";
import { StatusChip } from "./StatusChip";

type TaskRow = {
  id: string;
  title: string;
  state: string;
  owner_persona?: string | null;
  parent_task_id?: string | null;
};

type SpawnRule = {
  id: string;
  trigger_state: string;
  title_pattern?: string | null;
  owner_persona?: string | null;
  max_subtasks: number;
  subagent_persona: string;
  active: boolean;
};

type Handoff = {
  id: string;
  task_id: string;
  from_agent: string;
  to_agent: string;
  status: string;
  notes?: string | null;
  created_at: string;
};

type ImprovementRun = {
  id: string;
  title: string;
  scope: string;
  status: string;
  decision_reason?: string | null;
  created_at: string;
};

type ContractVersion = {
  id: string;
  contract_id: string;
  version: string;
  status: string;
};

type ConnectorPreset = {
  id: string;
  vertical: string;
  connector_provider: string;
};

type GoLiveItem = {
  id: string;
  item_key: string;
  item_label: string;
  required: boolean;
  completed: boolean;
  completed_by?: string | null;
};

export function OrchestrationPanels({
  api,
  companyId,
  tasks,
  setCoErr,
  loadCompanyOs,
}: {
  api: string;
  companyId: string;
  tasks: TaskRow[];
  setCoErr: (v: string | null) => void;
  loadCompanyOs: () => Promise<void>;
}) {
  const [rules, setRules] = useState<SpawnRule[]>([]);
  const [handoffs, setHandoffs] = useState<Handoff[]>([]);
  const [runs, setRuns] = useState<ImprovementRun[]>([]);
  const [contractVersions, setContractVersions] = useState<ContractVersion[]>([]);
  const [connectorPresets, setConnectorPresets] = useState<ConnectorPreset[]>([]);
  const [goLive, setGoLive] = useState<GoLiveItem[]>([]);

  const [ruleState, setRuleState] = useState("open");
  const [rulePattern, setRulePattern] = useState("");
  const [ruleOwner, setRuleOwner] = useState("");
  const [ruleMax, setRuleMax] = useState("2");
  const [rulePersona, setRulePersona] = useState("worker_agent");

  const [runTitle, setRunTitle] = useState("");
  const [runScope, setRunScope] = useState("");
  const [handoffNotes, setHandoffNotes] = useState<Record<string, string>>({});
  const [diagContract, setDiagContract] = useState("generic_smb_core_v1");
  const [diagTranscript, setDiagTranscript] = useState("");
  const [diagMissing, setDiagMissing] = useState<string[]>([]);
  const [goLiveKey, setGoLiveKey] = useState("");
  const [goLiveLabel, setGoLiveLabel] = useState("");
  const [newContractId, setNewContractId] = useState("");
  const [newContractVersion, setNewContractVersion] = useState("v1");
  const [newPresetVertical, setNewPresetVertical] = useState("generic_smb");
  const [newPresetProvider, setNewPresetProvider] = useState("email");
  const [seedVertical, setSeedVertical] = useState("generic_smb");

  const taskById = useMemo(() => new Map(tasks.map((t) => [t.id, t])), [tasks]);

  const taskChildren = useMemo(() => {
    const m = new Map<string, TaskRow[]>();
    for (const t of tasks) {
      const p = t.parent_task_id ? String(t.parent_task_id) : "";
      if (!p) continue;
      if (!m.has(p)) m.set(p, []);
      m.get(p)!.push(t);
    }
    return m;
  }, [tasks]);

  const rootTasks = useMemo(() => tasks.filter((t) => !t.parent_task_id), [tasks]);

  async function loadRules() {
    const r = await fetch(`${api}/api/company/companies/${companyId}/spawn-rules`);
    const j = (await r.json()) as { rules?: SpawnRule[]; error?: string };
    if (!r.ok) throw new Error(j.error ?? `spawn-rules ${r.status}`);
    setRules(j.rules ?? []);
  }

  async function loadHandoffs() {
    const all = await Promise.all(
      tasks.map(async (t) => {
        const r = await fetch(`${api}/api/company/companies/${companyId}/tasks/${t.id}/handoffs`);
        const j = (await r.json()) as { handoffs?: Handoff[]; error?: string };
        if (!r.ok) throw new Error(j.error ?? `handoffs ${r.status}`);
        return j.handoffs ?? [];
      })
    );
    setHandoffs(all.flat().filter((h) => h.status === "pending_review"));
  }

  async function loadRuns() {
    const r = await fetch(`${api}/api/company/companies/${companyId}/improvement-runs`);
    const j = (await r.json()) as { runs?: ImprovementRun[]; error?: string };
    if (!r.ok) throw new Error(j.error ?? `improvement-runs ${r.status}`);
    setRuns(j.runs ?? []);
  }

  async function loadContractVersions() {
    const r = await fetch(`${api}/api/company/contracts/versions`);
    const j = (await r.json()) as { versions?: ContractVersion[]; error?: string };
    if (!r.ok) throw new Error(j.error ?? `contracts/versions ${r.status}`);
    setContractVersions(j.versions ?? []);
  }

  async function loadConnectorPresets() {
    const r = await fetch(`${api}/api/company/connectors/presets`);
    const j = (await r.json()) as { presets?: ConnectorPreset[]; error?: string };
    if (!r.ok) throw new Error(j.error ?? `connectors/presets ${r.status}`);
    setConnectorPresets(j.presets ?? []);
  }

  async function loadGoLive() {
    const r = await fetch(`${api}/api/company/companies/${companyId}/go-live-checklist`);
    const j = (await r.json()) as { checklist?: GoLiveItem[]; error?: string };
    if (!r.ok) throw new Error(j.error ?? `go-live-checklist ${r.status}`);
    setGoLive(j.checklist ?? []);
  }

  async function refreshAll() {
    try {
      await Promise.all([
        loadRules(),
        loadHandoffs(),
        loadRuns(),
        loadContractVersions(),
        loadConnectorPresets(),
        loadGoLive(),
      ]);
    } catch (e) {
      setCoErr(e instanceof Error ? e.message : String(e));
    }
  }

  useEffect(() => {
    void refreshAll();
  }, [companyId, tasks.length]); // refresh when task count changes materially

  function renderTaskNode(t: TaskRow, depth: number, visiting: Set<string>): React.ReactNode {
    if (visiting.has(t.id)) return null;
    visiting.add(t.id);
    const kids = taskChildren.get(t.id) ?? [];
    return (
      <div key={t.id} className="space-y-1" style={{ marginLeft: depth * 14 }}>
        <div className="flex items-center justify-between gap-2 rounded border border-line bg-ink/40 px-2 py-1 text-xs">
          <span>
            {t.title} · <span className="text-gray-500">{t.owner_persona || "unassigned"}</span>
          </span>
          <span className="flex items-center gap-2">
            <StatusChip label={t.state.toUpperCase()} />
            <button
              type="button"
              className="rounded border border-line px-1.5 py-0.5 text-[10px] text-gray-300"
              onClick={async () => {
                setCoErr(null);
                try {
                  const r = await fetch(
                    `${api}/api/company/companies/${companyId}/tasks/${t.id}/spawn-subagents`,
                    {
                      method: "POST",
                      headers: { "Content-Type": "application/json" },
                      body: JSON.stringify({ actor: "admin_ui" }),
                    }
                  );
                  const j = await r.json();
                  if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                  await loadCompanyOs();
                  await refreshAll();
                } catch (e) {
                  setCoErr(e instanceof Error ? e.message : String(e));
                }
              }}
            >
              Spawn
            </button>
          </span>
        </div>
        {kids.map((k) => renderTaskNode(k, depth + 1, new Set(visiting)))}
      </div>
    );
  }

  return (
    <div className="mb-4 grid gap-4 md:grid-cols-2">
      <Panel title="Spawn rules">
        <div className="mb-2 grid grid-cols-2 gap-2 text-xs">
          <input className="rounded border border-line bg-ink px-2 py-1" value={ruleState} onChange={(e) => setRuleState(e.target.value)} placeholder="trigger_state" />
          <input className="rounded border border-line bg-ink px-2 py-1" value={rulePersona} onChange={(e) => setRulePersona(e.target.value)} placeholder="subagent_persona" />
          <input className="rounded border border-line bg-ink px-2 py-1" value={rulePattern} onChange={(e) => setRulePattern(e.target.value)} placeholder="title_pattern (optional)" />
          <input className="rounded border border-line bg-ink px-2 py-1" value={ruleOwner} onChange={(e) => setRuleOwner(e.target.value)} placeholder="owner_persona (optional)" />
          <input className="rounded border border-line bg-ink px-2 py-1" value={ruleMax} onChange={(e) => setRuleMax(e.target.value)} placeholder="max_subtasks" />
          <button
            type="button"
            className="rounded bg-accent/20 px-2 py-1 text-accent"
            onClick={async () => {
              setCoErr(null);
              try {
                const n = Number(ruleMax.trim());
                const r = await fetch(`${api}/api/company/companies/${companyId}/spawn-rules`, {
                  method: "POST",
                  headers: { "Content-Type": "application/json" },
                  body: JSON.stringify({
                    trigger_state: ruleState.trim() || "open",
                    title_pattern: rulePattern.trim() || undefined,
                    owner_persona: ruleOwner.trim() || undefined,
                    max_subtasks: Number.isFinite(n) ? n : 2,
                    subagent_persona: rulePersona.trim() || "worker_agent",
                    handoff_contract: { required_fields: ["summary", "deliverable"] },
                    review_contract: { reviewer_role: "admin", checks: ["quality", "risk"] },
                  }),
                });
                const j = await r.json();
                if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                setRulePattern("");
                setRuleOwner("");
                await refreshAll();
              } catch (e) {
                setCoErr(e instanceof Error ? e.message : String(e));
              }
            }}
          >
            Add rule
          </button>
        </div>
        <div className="max-h-[180px] space-y-1 overflow-auto text-xs text-gray-400">
          {rules.map((r) => (
            <div key={r.id} className="rounded border border-line bg-ink/40 px-2 py-1">
              {r.trigger_state} {"->"} {r.subagent_persona} x{r.max_subtasks}
              {r.title_pattern ? ` · title~${r.title_pattern}` : ""}
              {r.owner_persona ? ` · owner=${r.owner_persona}` : ""}
            </div>
          ))}
          {!rules.length && <div className="text-gray-600">No spawn rules yet.</div>}
        </div>
      </Panel>

      <Panel title="Handoff review queue">
        <div className="max-h-[220px] space-y-1 overflow-auto text-xs text-gray-400">
          {handoffs.map((h) => (
            <div key={h.id} className="rounded border border-line bg-ink/40 px-2 py-1">
              <div>
                {taskById.get(h.task_id)?.title ?? h.task_id} · {h.from_agent} {"->"} {h.to_agent}
              </div>
              <div className="mt-1 flex items-center gap-2">
                <input
                  className="min-w-0 flex-1 rounded border border-line bg-ink px-2 py-1 text-[11px]"
                  placeholder="review notes"
                  value={handoffNotes[h.id] ?? ""}
                  onChange={(e) => setHandoffNotes((m) => ({ ...m, [h.id]: e.target.value }))}
                />
                <button
                  type="button"
                  className="rounded border border-emerald-700 px-2 py-1 text-[11px] text-emerald-300"
                  onClick={async () => {
                    setCoErr(null);
                    try {
                      const r = await fetch(`${api}/api/company/task-handoffs/${h.id}/review`, {
                        method: "POST",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({
                          decision: "accept",
                          reviewer: "admin_ui",
                          notes: handoffNotes[h.id] ?? "",
                        }),
                      });
                      const j = await r.json();
                      if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                      await refreshAll();
                    } catch (e) {
                      setCoErr(e instanceof Error ? e.message : String(e));
                    }
                  }}
                >
                  Accept
                </button>
                <button
                  type="button"
                  className="rounded border border-red-800 px-2 py-1 text-[11px] text-red-300"
                  onClick={async () => {
                    setCoErr(null);
                    try {
                      const r = await fetch(`${api}/api/company/task-handoffs/${h.id}/review`, {
                        method: "POST",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({
                          decision: "reject",
                          reviewer: "admin_ui",
                          notes: handoffNotes[h.id] ?? "",
                        }),
                      });
                      const j = await r.json();
                      if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                      await refreshAll();
                    } catch (e) {
                      setCoErr(e instanceof Error ? e.message : String(e));
                    }
                  }}
                >
                  Reject
                </button>
              </div>
            </div>
          ))}
          {!handoffs.length && <div className="text-gray-600">No pending handoffs.</div>}
        </div>
      </Panel>

      <Panel title="Improvement runs">
        <div className="mb-2 grid grid-cols-2 gap-2 text-xs">
          <input className="rounded border border-line bg-ink px-2 py-1" value={runTitle} onChange={(e) => setRunTitle(e.target.value)} placeholder="title" />
          <input className="rounded border border-line bg-ink px-2 py-1" value={runScope} onChange={(e) => setRunScope(e.target.value)} placeholder="scope" />
          <button
            type="button"
            className="rounded bg-accent/20 px-2 py-1 text-accent"
            onClick={async () => {
              setCoErr(null);
              try {
                const r = await fetch(`${api}/api/company/companies/${companyId}/improvement-runs`, {
                  method: "POST",
                  headers: { "Content-Type": "application/json" },
                  body: JSON.stringify({
                    title: runTitle.trim(),
                    scope: runScope.trim(),
                    gate_contract: { min_eval_samples: 20, max_regression_pct: 3 },
                  }),
                });
                const j = await r.json();
                if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                setRunTitle("");
                setRunScope("");
                await refreshAll();
              } catch (e) {
                setCoErr(e instanceof Error ? e.message : String(e));
              }
            }}
          >
            Propose
          </button>
        </div>
        <div className="max-h-[180px] space-y-1 overflow-auto text-xs text-gray-400">
          {runs.map((r) => (
            <div key={r.id} className="rounded border border-line bg-ink/40 px-2 py-1">
              <div className="mb-1 flex items-center justify-between gap-2">
                <span>{r.title} · {r.scope}</span>
                <StatusChip label={r.status.toUpperCase()} />
              </div>
              <div className="flex gap-2">
                <button
                  type="button"
                  className="rounded border border-emerald-700 px-2 py-0.5 text-[11px] text-emerald-300"
                  onClick={async () => {
                    setCoErr(null);
                    try {
                      const res = await fetch(`${api}/api/company/improvement-runs/${r.id}/decision`, {
                        method: "POST",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({ decision: "promote", actor: "admin_ui", reason: "meets gates" }),
                      });
                      const j = await res.json();
                      if (!res.ok) throw new Error((j as { error?: string }).error ?? res.statusText);
                      await refreshAll();
                    } catch (e) {
                      setCoErr(e instanceof Error ? e.message : String(e));
                    }
                  }}
                >
                  Promote
                </button>
                <button
                  type="button"
                  className="rounded border border-red-800 px-2 py-0.5 text-[11px] text-red-300"
                  onClick={async () => {
                    setCoErr(null);
                    try {
                      const res = await fetch(`${api}/api/company/improvement-runs/${r.id}/decision`, {
                        method: "POST",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({ decision: "revert", actor: "admin_ui", reason: "regression risk" }),
                      });
                      const j = await res.json();
                      if (!res.ok) throw new Error((j as { error?: string }).error ?? res.statusText);
                      await refreshAll();
                    } catch (e) {
                      setCoErr(e instanceof Error ? e.message : String(e));
                    }
                  }}
                >
                  Revert
                </button>
              </div>
            </div>
          ))}
          {!runs.length && <div className="text-gray-600">No improvement runs yet.</div>}
        </div>
      </Panel>

      <Panel title="Task graph">
        <div className="max-h-[240px] space-y-1 overflow-auto text-xs text-gray-300">
          {rootTasks.map((t) => renderTaskNode(t, 0, new Set()))}
          {!rootTasks.length && <div className="text-gray-600">No tasks graph yet.</div>}
        </div>
      </Panel>

      <Panel title="Contract gate diagnostics">
        <div className="mb-2 grid grid-cols-2 gap-2 text-xs">
          <input
            className="rounded border border-line bg-ink px-2 py-1"
            value={diagContract}
            onChange={(e) => setDiagContract(e.target.value)}
            placeholder="pack_contract_id"
          />
          <button
            type="button"
            className="rounded border border-line px-2 py-1 text-gray-300"
            onClick={async () => {
              setCoErr(null);
              setDiagMissing([]);
              try {
                const r = await fetch(`${api}/api/company/onboarding/contracts/validate`, {
                  method: "POST",
                  headers: { "Content-Type": "application/json" },
                  body: JSON.stringify({
                    pack_contract_id: diagContract.trim(),
                    transcript: diagTranscript,
                  }),
                });
                const j = (await r.json()) as { unsatisfied_required_gates?: string[]; error?: string };
                if (!r.ok) throw new Error(j.error ?? r.statusText);
                setDiagMissing(j.unsatisfied_required_gates ?? []);
              } catch (e) {
                setCoErr(e instanceof Error ? e.message : String(e));
              }
            }}
          >
            Diagnose
          </button>
        </div>
        <textarea
          className="mb-2 min-h-[90px] w-full rounded border border-line bg-ink px-2 py-1 text-xs"
          value={diagTranscript}
          onChange={(e) => setDiagTranscript(e.target.value)}
          placeholder="Paste onboarding transcript or policy notes..."
        />
        <div className="text-xs text-gray-400">
          {diagMissing.length ? (
            <div className="text-amber-300">Missing evidence gates: {diagMissing.join(", ")}</div>
          ) : (
            <div className="text-emerald-300">No missing required gates.</div>
          )}
        </div>
      </Panel>

      <Panel title="Scale pack">
        <div className="mb-2 text-[11px] uppercase text-gray-500">Contract versions</div>
        <div className="mb-2 grid grid-cols-2 gap-2 text-xs">
          <input className="rounded border border-line bg-ink px-2 py-1" value={newContractId} onChange={(e) => setNewContractId(e.target.value)} placeholder="contract_id" />
          <input className="rounded border border-line bg-ink px-2 py-1" value={newContractVersion} onChange={(e) => setNewContractVersion(e.target.value)} placeholder="version (v1.1)" />
          <button
            type="button"
            className="rounded bg-accent/20 px-2 py-1 text-accent"
            onClick={async () => {
              setCoErr(null);
              try {
                const r = await fetch(`${api}/api/company/contracts/versions`, {
                  method: "POST",
                  headers: { "Content-Type": "application/json" },
                  body: JSON.stringify({
                    contract_id: newContractId.trim(),
                    version: newContractVersion.trim(),
                    status: "active",
                    schema: {},
                  }),
                });
                const j = await r.json();
                if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                setNewContractId("");
                await loadContractVersions();
              } catch (e) {
                setCoErr(e instanceof Error ? e.message : String(e));
              }
            }}
          >
            Add version
          </button>
        </div>
        <div className="mb-2 max-h-[80px] space-y-1 overflow-auto text-xs text-gray-400">
          {contractVersions.map((v) => (
            <div key={v.id} className="flex items-center justify-between gap-2 rounded border border-line bg-ink/40 px-2 py-1">
              <span>{v.contract_id} · {v.version} · {v.status}</span>
              <span className="flex gap-1">
                <button
                  type="button"
                  className="rounded border border-line px-1 py-0.5 text-[10px] text-gray-300"
                  onClick={async () => {
                    setCoErr(null);
                    try {
                      const r = await fetch(`${api}/api/company/contracts/versions/${v.id}/status`, {
                        method: "PATCH",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({ status: "deprecated" }),
                      });
                      const j = await r.json();
                      if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                      await loadContractVersions();
                    } catch (e) {
                      setCoErr(e instanceof Error ? e.message : String(e));
                    }
                  }}
                >
                  Deprecate
                </button>
                <button
                  type="button"
                  className="rounded border border-line px-1 py-0.5 text-[10px] text-gray-300"
                  onClick={async () => {
                    setCoErr(null);
                    try {
                      const r = await fetch(`${api}/api/company/contracts/versions/${v.id}/status`, {
                        method: "PATCH",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({ status: "sunset" }),
                      });
                      const j = await r.json();
                      if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                      await loadContractVersions();
                    } catch (e) {
                      setCoErr(e instanceof Error ? e.message : String(e));
                    }
                  }}
                >
                  Sunset
                </button>
              </span>
            </div>
          ))}
          {!contractVersions.length && <div className="text-gray-600">No contract versions loaded.</div>}
        </div>

        <div className="mb-2 text-[11px] uppercase text-gray-500">Connector presets</div>
        <div className="mb-2 grid grid-cols-2 gap-2 text-xs">
          <input className="rounded border border-line bg-ink px-2 py-1" value={newPresetVertical} onChange={(e) => setNewPresetVertical(e.target.value)} placeholder="vertical" />
          <input className="rounded border border-line bg-ink px-2 py-1" value={newPresetProvider} onChange={(e) => setNewPresetProvider(e.target.value)} placeholder="provider" />
          <button
            type="button"
            className="rounded bg-accent/20 px-2 py-1 text-accent"
            onClick={async () => {
              setCoErr(null);
              try {
                const r = await fetch(`${api}/api/company/connectors/presets`, {
                  method: "POST",
                  headers: { "Content-Type": "application/json" },
                  body: JSON.stringify({
                    vertical: newPresetVertical.trim(),
                    connector_provider: newPresetProvider.trim(),
                    allowed_actions: ["read", "write"],
                    blocked_actions: ["delete_all"],
                  }),
                });
                const j = await r.json();
                if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                await loadConnectorPresets();
              } catch (e) {
                setCoErr(e instanceof Error ? e.message : String(e));
              }
            }}
          >
            Upsert preset
          </button>
        </div>
        <div className="mb-2 max-h-[80px] space-y-1 overflow-auto text-xs text-gray-400">
          {connectorPresets.map((p) => (
            <div key={p.id} className="rounded border border-line bg-ink/40 px-2 py-1">
              {p.vertical} · {p.connector_provider}
            </div>
          ))}
          {!connectorPresets.length && <div className="text-gray-600">No connector presets configured.</div>}
        </div>

        <div className="mb-2 text-[11px] uppercase text-gray-500">Go-live checklist</div>
        <div className="mb-2 flex gap-2 text-xs">
          <select
            className="rounded border border-line bg-ink px-2 py-1"
            value={seedVertical}
            onChange={(e) => setSeedVertical(e.target.value)}
          >
            <option value="generic_smb">generic_smb</option>
            <option value="ecommerce">ecommerce</option>
            <option value="property_management">property_management</option>
          </select>
          <button
            type="button"
            className="rounded border border-line px-2 py-1 text-gray-300"
            onClick={async () => {
              setCoErr(null);
              try {
                const r = await fetch(`${api}/api/company/companies/${companyId}/go-live-checklist/seed`, {
                  method: "POST",
                  headers: { "Content-Type": "application/json" },
                  body: JSON.stringify({ vertical: seedVertical, actor: "admin_ui" }),
                });
                const j = await r.json();
                if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                await loadGoLive();
              } catch (e) {
                setCoErr(e instanceof Error ? e.message : String(e));
              }
            }}
          >
            Seed template
          </button>
        </div>
        <div className="mb-2 grid grid-cols-2 gap-2 text-xs">
          <input
            className="rounded border border-line bg-ink px-2 py-1"
            value={goLiveKey}
            onChange={(e) => setGoLiveKey(e.target.value)}
            placeholder="item_key"
          />
          <input
            className="rounded border border-line bg-ink px-2 py-1"
            value={goLiveLabel}
            onChange={(e) => setGoLiveLabel(e.target.value)}
            placeholder="item_label"
          />
          <button
            type="button"
            className="rounded bg-accent/20 px-2 py-1 text-accent"
            onClick={async () => {
              setCoErr(null);
              try {
                const r = await fetch(`${api}/api/company/companies/${companyId}/go-live-checklist`, {
                  method: "POST",
                  headers: { "Content-Type": "application/json" },
                  body: JSON.stringify({
                    item_key: goLiveKey.trim(),
                    item_label: goLiveLabel.trim(),
                    required: true,
                  }),
                });
                const j = await r.json();
                if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                setGoLiveKey("");
                setGoLiveLabel("");
                await loadGoLive();
              } catch (e) {
                setCoErr(e instanceof Error ? e.message : String(e));
              }
            }}
          >
            Add checklist item
          </button>
        </div>
        <div className="max-h-[120px] space-y-1 overflow-auto text-xs text-gray-400">
          {goLive.map((i) => (
            <div key={i.id} className="flex items-center justify-between gap-2 rounded border border-line bg-ink/40 px-2 py-1">
              <span>
                {i.item_label} {i.required ? "(required)" : ""}
              </span>
              {i.completed ? (
                <StatusChip label="DONE" tone="green" />
              ) : (
                <button
                  type="button"
                  className="rounded border border-line px-1.5 py-0.5 text-[10px] text-gray-300"
                  onClick={async () => {
                    setCoErr(null);
                    try {
                      const r = await fetch(`${api}/api/company/go-live-checklist/${i.id}/complete`, {
                        method: "POST",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({ actor: "admin_ui" }),
                      });
                      const j = await r.json();
                      if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                      await loadGoLive();
                    } catch (e) {
                      setCoErr(e instanceof Error ? e.message : String(e));
                    }
                  }}
                >
                  Complete
                </button>
              )}
            </div>
          ))}
          {!goLive.length && <div className="text-gray-600">No go-live checklist items yet.</div>}
        </div>
      </Panel>
    </div>
  );
}

