"use client";

import { ChevronRight } from "lucide-react";
import { useCallback, useEffect, useId, useMemo, useState } from "react";

import {
  EXAMPLE_WORKFORCE_AGENT_PRESETS,
  type ExampleWorkforceAgentPreset,
  type ExampleWorkforceAgentSource,
  exampleWorkforceAgentNames,
} from "@/app/lib/example-company-agents";

export type CoAgentRow = {
  id: string;
  name: string;
  role: string;
  title?: string | null;
  capabilities?: string | null;
  reports_to?: string | null;
  adapter_type?: string | null;
  adapter_config?: unknown;
  budget_monthly_cents?: number | null;
  briefing?: string | null;
  status: string;
  sort_order: number;
};

const PRESET_SOURCE_LABEL: Record<ExampleWorkforceAgentSource, string> = {
  paperclip: "Paperclip template roster",
  hermes: "Hermes bridge",
  sop: "SOP demo personas",
};

type Props = {
  api: string;
  companyId: string;
  agents: CoAgentRow[];
  /** Distinct owner_persona / checked_out_by from tasks — drives suggestion dropdown. */
  suggestedAgentIds?: string[];
  setCoErr: (msg: string | null) => void;
  loadCompanyOs: () => Promise<void>;
};

function adapterConfigStr(cfg: unknown): string {
  if (cfg === undefined || cfg === null) return "{}";
  try {
    return JSON.stringify(cfg, null, 2);
  } catch {
    return "{}";
  }
}

export function CompanyAgentsPanel({
  api,
  companyId,
  agents,
  suggestedAgentIds = [],
  setCoErr,
  loadCompanyOs,
}: Props) {
  const agentIdDatalistId = useId().replace(/:/g, "");
  const [expanded, setExpanded] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("");
  const [newRole, setNewRole] = useState("worker");
  const [newTitle, setNewTitle] = useState("");
  const [newBriefing, setNewBriefing] = useState("");
  const [newCapabilities, setNewCapabilities] = useState("");
  const [newReportsTo, setNewReportsTo] = useState("");
  const [newAdapterType, setNewAdapterType] = useState("");
  const [newAdapterJson, setNewAdapterJson] = useState("{}");
  const [newBudgetDollars, setNewBudgetDollars] = useState("");

  const byId = useMemo(() => new Map(agents.map((a) => [a.id, a])), [agents]);

  const taskPersonaSet = useMemo(
    () => new Set(suggestedAgentIds.map((s) => s.trim()).filter(Boolean)),
    [suggestedAgentIds]
  );

  const exampleNames = useMemo(() => exampleWorkforceAgentNames(), []);

  const presetsBySource = useMemo(() => {
    const m: Record<ExampleWorkforceAgentSource, ExampleWorkforceAgentPreset[]> = {
      paperclip: [],
      hermes: [],
      sop: [],
    };
    for (const p of EXAMPLE_WORKFORCE_AGENT_PRESETS) m[p.source].push(p);
    return m;
  }, []);

  const mergedIdSuggestions = useMemo(() => {
    const set = new Set<string>();
    for (const x of exampleNames) set.add(x);
    for (const x of suggestedAgentIds) {
      const t = x.trim();
      if (t) set.add(t);
    }
    return [...set].sort((a, b) => a.localeCompare(b));
  }, [exampleNames, suggestedAgentIds]);

  const applyExamplePreset = useCallback((p: ExampleWorkforceAgentPreset) => {
    setNewName(p.name);
    setNewRole(p.role);
    setNewTitle(p.title ?? "");
    setNewBriefing(p.briefing ?? "");
    setNewCapabilities(p.capabilities ?? "");
    setNewAdapterType(p.adapter_type ?? "");
    setNewAdapterJson(JSON.stringify(p.adapter_config ?? {}, null, 2));
  }, []);

  const reload = useCallback(async () => {
    await loadCompanyOs();
  }, [loadCompanyOs]);

  useEffect(() => {
    setExpanded(null);
    setCreating(false);
  }, [companyId]);

  const createAgent = async () => {
    const name = newName.trim();
    if (!name) {
      setCoErr("Agent id (name) is required — use letters, digits, _ or - only.");
      return;
    }
    let adapter_config: unknown = {};
    try {
      adapter_config = JSON.parse(newAdapterJson.trim() || "{}");
    } catch {
      setCoErr("Adapter config must be valid JSON.");
      return;
    }
    const budget_trim = newBudgetDollars.trim();
    const budget_monthly_cents =
      budget_trim === "" ? undefined : Math.round(parseFloat(budget_trim) * 100);
    if (budget_trim !== "" && !Number.isFinite(budget_monthly_cents)) {
      setCoErr("Budget must be a number (monthly USD).");
      return;
    }
    setCoErr(null);
    try {
      const r = await fetch(`${api}/api/company/companies/${companyId}/agents`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          name,
          role: newRole.trim() || "worker",
          title: newTitle.trim() || undefined,
          capabilities: newCapabilities.trim() || undefined,
          briefing: newBriefing.trim() || undefined,
          reports_to: newReportsTo.trim() || undefined,
          adapter_type: newAdapterType.trim() || undefined,
          adapter_config,
          budget_monthly_cents,
        }),
      });
      const j = (await r.json()) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? r.statusText);
      setNewName("");
      setNewRole("worker");
      setNewTitle("");
      setNewBriefing("");
      setNewCapabilities("");
      setNewReportsTo("");
      setNewAdapterType("");
      setNewAdapterJson("{}");
      setNewBudgetDollars("");
      setCreating(false);
      await reload();
    } catch (e) {
      setCoErr(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <details className="group mb-6 rounded-xl border border-[#30363D] bg-[#0d1117]" open>
      <summary className="flex cursor-pointer list-none items-start gap-2 px-4 py-3.5 marker:content-none [&::-webkit-details-marker]:hidden">
        <ChevronRight
          className="mt-0.5 h-4 w-4 shrink-0 text-[#8B949E] transition-transform duration-200 group-open:rotate-90"
          aria-hidden
        />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-sm font-medium text-white">Workforce roster</span>
            <span className="rounded border border-[#a371f7]/35 bg-[#a371f7]/10 px-2 py-px font-mono text-[10px] font-semibold uppercase tracking-wide text-[#d2a8ff]">
              Paperclip-style
            </span>
          </div>
          <p className="mt-1 text-xs leading-relaxed text-[#8B949E]">
            People-shaped rows: org chart, adapters, budgets. Agent <strong className="text-[#c9d1d9]">id</strong> must
            match <code className="text-[11px] text-[#79b8ff]">owner_persona</code> / checkout names on tasks.
          </p>
        </div>
      </summary>
      <div className="space-y-4 border-t border-[#30363D] px-4 py-4">
        <p className="text-sm leading-relaxed text-[#8B949E]">
          <strong className="text-[#c9d1d9]">Role</strong> is org position (<code className="text-[11px]">worker</code>,{" "}
          <code className="text-[11px]">manager</code>, …), not the id. Presets mirror Paperclip rosters, Hermes{" "}
          <code className="text-[11px] text-[#79b8ff]">adapter_type=hermes</code>, and SOP catalog examples.
        </p>

        {!creating ? (
          <button
            type="button"
            className="rounded-full bg-white px-4 py-2 text-sm font-medium text-black hover:bg-gray-200"
            onClick={() => setCreating(true)}
          >
            Add agent
          </button>
        ) : (
          <div className="rounded-xl border border-dashed border-line/80 bg-black/20 p-4">
            <div className="mb-2 text-xs font-medium uppercase tracking-wide text-gray-500">New agent</div>
            <datalist id={agentIdDatalistId}>
              {mergedIdSuggestions.map((id) => (
                <option key={id} value={id} />
              ))}
            </datalist>
            <div className="mb-3">
              <label className="mb-1 block text-xs text-gray-500">
                Example roster preset <span className="text-gray-600">(fills form — Paperclip / Hermes / SOP)</span>
              </label>
              <select
                className="w-full max-w-md rounded-lg border border-line bg-ink px-3 py-2 text-sm text-gray-200"
                value=""
                onChange={(e) => {
                  const v = e.target.value;
                  if (!v) return;
                  const preset = EXAMPLE_WORKFORCE_AGENT_PRESETS.find((p) => p.name === v);
                  if (preset) applyExamplePreset(preset);
                  e.currentTarget.value = "";
                }}
                aria-label="Apply example workforce preset"
              >
                <option value="">— Choose preset —</option>
                {(["paperclip", "hermes", "sop"] as const).map((src) => (
                  <optgroup key={src} label={PRESET_SOURCE_LABEL[src]}>
                    {presetsBySource[src].map((p) => (
                      <option key={p.name} value={p.name}>
                        {p.name}
                        {p.title ? ` — ${p.title}` : ""}
                      </option>
                    ))}
                  </optgroup>
                ))}
              </select>
            </div>
            <div className="mb-3">
              <label className="mb-1 block text-xs text-gray-500">
                Start from a suggestion <span className="text-gray-600">(optional)</span>
              </label>
              <select
                className="w-full max-w-md rounded-lg border border-line bg-ink px-3 py-2 text-sm text-gray-200"
                value=""
                onChange={(e) => {
                  const v = e.target.value;
                  if (v) setNewName(v);
                }}
                aria-label="Insert suggested agent id"
              >
                <option value="">— Choose id preset or task persona —</option>
                {mergedIdSuggestions.map((id) => (
                  <option key={id} value={id}>
                    {id}
                    {taskPersonaSet.has(id) ? " (from your tasks)" : " (example)"}
                  </option>
                ))}
              </select>
            </div>
            <div className="grid gap-2 sm:grid-cols-2">
              <div className="flex flex-col gap-1">
                <label className="text-xs font-medium text-gray-400" htmlFor="co-new-agent-id">
                  Agent id <span className="font-normal text-gray-600">(matches tasks / checkout)</span>
                </label>
                <input
                  id="co-new-agent-id"
                  className="rounded-lg border border-line bg-ink px-3 py-2 font-mono text-sm"
                  placeholder="e.g. property_admin"
                  list={agentIdDatalistId}
                  autoComplete="off"
                  value={newName}
                  onChange={(e) => setNewName(e.target.value)}
                />
              </div>
              <div className="flex flex-col gap-1">
                <label className="text-xs font-medium text-gray-400" htmlFor="co-new-agent-role">
                  Role <span className="font-normal text-gray-600">(org chart, not the id)</span>
                </label>
                <input
                  id="co-new-agent-role"
                  className="rounded-lg border border-line bg-ink px-3 py-2 text-sm"
                  placeholder="worker, manager, …"
                  value={newRole}
                  onChange={(e) => setNewRole(e.target.value)}
                />
              </div>
              <input
                className="rounded-lg border border-line bg-ink px-3 py-2 text-sm sm:col-span-2"
                placeholder="Title (e.g. Senior Property Coordinator)"
                value={newTitle}
                onChange={(e) => setNewTitle(e.target.value)}
              />
              <select
                className="rounded-lg border border-line bg-ink px-3 py-2 text-sm sm:col-span-2"
                value={newReportsTo}
                onChange={(e) => setNewReportsTo(e.target.value)}
              >
                <option value="">Reports to — (top of org)</option>
                {agents.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name} · {a.role}
                  </option>
                ))}
              </select>
              <textarea
                className="min-h-[72px] rounded-lg border border-line bg-ink px-3 py-2 text-sm sm:col-span-2"
                placeholder="Capabilities (skills, tools, domains)"
                value={newCapabilities}
                onChange={(e) => setNewCapabilities(e.target.value)}
              />
              <textarea
                className="min-h-[96px] rounded-lg border border-line bg-ink px-3 py-2 text-sm sm:col-span-2"
                placeholder="Briefing — what they need to know and do day-to-day"
                value={newBriefing}
                onChange={(e) => setNewBriefing(e.target.value)}
              />
              <input
                className="rounded-lg border border-line bg-ink px-3 py-2 font-mono text-sm"
                placeholder="Adapter type (e.g. hermes, ollama, claude_local)"
                value={newAdapterType}
                onChange={(e) => setNewAdapterType(e.target.value)}
              />
              <input
                className="rounded-lg border border-line bg-ink px-3 py-2 text-sm"
                placeholder="Monthly budget USD (optional)"
                value={newBudgetDollars}
                onChange={(e) => setNewBudgetDollars(e.target.value)}
              />
              <textarea
                className="min-h-[80px] rounded-lg border border-line bg-ink px-3 py-2 font-mono text-xs sm:col-span-2"
                placeholder='Adapter JSON config e.g. {"model":"llama3"}'
                value={newAdapterJson}
                onChange={(e) => setNewAdapterJson(e.target.value)}
              />
            </div>
            <div className="mt-3 flex flex-wrap gap-2">
              <button
                type="button"
                className="rounded-full bg-accent/20 px-4 py-2 text-sm text-accent"
                onClick={() => void createAgent()}
              >
                Create
              </button>
              <button
                type="button"
                className="rounded-full border border-line px-4 py-2 text-sm text-gray-400"
                onClick={() => setCreating(false)}
              >
                Cancel
              </button>
            </div>
          </div>
        )}

        {agents.length === 0 && !creating ? (
          <p className="text-sm text-gray-600">No agents yet. Add one to match your task owner personas.</p>
        ) : null}

        <ul className="space-y-2">
          {agents.map((a) => (
            <AgentEditorRow
              key={a.id}
              api={api}
              companyId={companyId}
              agent={a}
              peers={agents}
              byId={byId}
              expanded={expanded === a.id}
              onToggle={() => setExpanded((x) => (x === a.id ? null : a.id))}
              setCoErr={setCoErr}
              onSaved={reload}
            />
          ))}
        </ul>

        <a
          href={`${api}/api/company/companies/${companyId}/org`}
          target="_blank"
          rel="noreferrer"
          className="inline-block text-xs text-accent hover:underline"
        >
          View org JSON (tree + flat)
        </a>
      </div>
    </details>
  );
}

function AgentEditorRow({
  api,
  companyId,
  agent,
  peers,
  byId,
  expanded,
  onToggle,
  setCoErr,
  onSaved,
}: {
  api: string;
  companyId: string;
  agent: CoAgentRow;
  peers: CoAgentRow[];
  byId: Map<string, CoAgentRow>;
  expanded: boolean;
  onToggle: () => void;
  setCoErr: (msg: string | null) => void;
  onSaved: () => Promise<void>;
}) {
  const [role, setRole] = useState(agent.role);
  const [title, setTitle] = useState(agent.title ?? "");
  const [capabilities, setCapabilities] = useState(agent.capabilities ?? "");
  const [briefing, setBriefing] = useState(agent.briefing ?? "");
  const [reportsTo, setReportsTo] = useState(agent.reports_to ?? "");
  const [adapterType, setAdapterType] = useState(agent.adapter_type ?? "");
  const [adapterJson, setAdapterJson] = useState(() => adapterConfigStr(agent.adapter_config));
  const [budgetDollars, setBudgetDollars] = useState(
    agent.budget_monthly_cents != null ? String(agent.budget_monthly_cents / 100) : ""
  );
  const [status, setStatus] = useState(agent.status);
  const [sortOrder, setSortOrder] = useState(String(agent.sort_order));
  const [saving, setSaving] = useState(false);
  const [removing, setRemoving] = useState(false);

  useEffect(() => {
    setRole(agent.role);
    setTitle(agent.title ?? "");
    setCapabilities(agent.capabilities ?? "");
    setBriefing(agent.briefing ?? "");
    setReportsTo(agent.reports_to ?? "");
    setAdapterType(agent.adapter_type ?? "");
    setAdapterJson(adapterConfigStr(agent.adapter_config));
    setBudgetDollars(agent.budget_monthly_cents != null ? String(agent.budget_monthly_cents / 100) : "");
    setStatus(agent.status);
    setSortOrder(String(agent.sort_order));
  }, [agent]);

  const managerLabel = agent.reports_to ? byId.get(agent.reports_to)?.name ?? "—" : "—";

  const save = async () => {
    let adapter_config: unknown;
    try {
      adapter_config = JSON.parse(adapterJson.trim() || "{}");
    } catch {
      setCoErr("Adapter config must be valid JSON.");
      return;
    }
    const btrim = budgetDollars.trim();
    let budget_monthly_cents: unknown = undefined;
    if (btrim === "") {
      budget_monthly_cents = null;
    } else {
      const c = Math.round(parseFloat(btrim) * 100);
      if (!Number.isFinite(c)) {
        setCoErr("Budget must be a number.");
        return;
      }
      budget_monthly_cents = c;
    }
    const so = parseInt(sortOrder, 10);
    if (!Number.isFinite(so)) {
      setCoErr("Sort order must be an integer.");
      return;
    }
    setCoErr(null);
    setSaving(true);
    try {
      const body: Record<string, unknown> = {
        role: role.trim(),
        title: title.trim() === "" ? null : title.trim(),
        capabilities: capabilities.trim() === "" ? null : capabilities.trim(),
        briefing: briefing.trim() === "" ? null : briefing.trim(),
        reports_to: reportsTo === "" ? null : reportsTo,
        adapter_type: adapterType.trim() === "" ? null : adapterType.trim(),
        adapter_config,
        budget_monthly_cents,
        status: status.trim() || "active",
        sort_order: so,
      };
      const r = await fetch(`${api}/api/company/companies/${companyId}/agents/${agent.id}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      const j = (await r.json()) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? r.statusText);
      await onSaved();
    } catch (e) {
      setCoErr(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const removeAgent = async () => {
    if (
      !window.confirm(
        `Remove workforce agent "${agent.name}" from this company?\n\nDirect reports move to the top of the org (their manager link is cleared). Tasks that still reference this name as owner_persona are unchanged.`
      )
    ) {
      return;
    }
    setCoErr(null);
    setRemoving(true);
    try {
      const r = await fetch(`${api}/api/company/companies/${companyId}/agents/${agent.id}`, {
        method: "DELETE",
      });
      const j = (await r.json()) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? r.statusText);
      await onSaved();
    } catch (e) {
      setCoErr(e instanceof Error ? e.message : String(e));
    } finally {
      setRemoving(false);
    }
  };

  return (
    <li className="rounded-lg border border-line bg-ink/30">
      <button
        type="button"
        className="flex w-full items-center justify-between gap-2 px-3 py-2 text-left"
        onClick={onToggle}
      >
        <span className="font-mono text-sm text-accent">{agent.name}</span>
        <span className="text-xs text-gray-500">
          {agent.role}
          {agent.title ? ` · ${agent.title}` : ""} · reports to {managerLabel} · {agent.status}
        </span>
      </button>
      {expanded ? (
        <div className="border-t border-line/60 px-3 py-3">
          <div className="grid gap-2 sm:grid-cols-2">
            <label className="sm:col-span-2 text-xs text-gray-500">
              Role
              <input
                className="mt-1 w-full rounded border border-line bg-ink px-2 py-1.5 text-sm"
                value={role}
                onChange={(e) => setRole(e.target.value)}
              />
            </label>
            <label className="sm:col-span-2 text-xs text-gray-500">
              Title
              <input
                className="mt-1 w-full rounded border border-line bg-ink px-2 py-1.5 text-sm"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
              />
            </label>
            <label className="sm:col-span-2 text-xs text-gray-500">
              Reports to
              <select
                className="mt-1 w-full rounded border border-line bg-ink px-2 py-1.5 text-sm"
                value={reportsTo}
                onChange={(e) => setReportsTo(e.target.value)}
              >
                <option value="">(top of org)</option>
                {peers
                  .filter((p) => p.id !== agent.id)
                  .map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name}
                    </option>
                  ))}
              </select>
            </label>
            <label className="sm:col-span-2 text-xs text-gray-500">
              Capabilities
              <textarea
                className="mt-1 min-h-[64px] w-full rounded border border-line bg-ink px-2 py-1.5 text-sm"
                value={capabilities}
                onChange={(e) => setCapabilities(e.target.value)}
              />
            </label>
            <label className="sm:col-span-2 text-xs text-gray-500">
              Briefing (know / do)
              <textarea
                className="mt-1 min-h-[88px] w-full rounded border border-line bg-ink px-2 py-1.5 text-sm"
                value={briefing}
                onChange={(e) => setBriefing(e.target.value)}
              />
            </label>
            <label className="text-xs text-gray-500">
              Adapter type
              <input
                className="mt-1 w-full rounded border border-line bg-ink px-2 py-1.5 font-mono text-sm"
                value={adapterType}
                onChange={(e) => setAdapterType(e.target.value)}
              />
            </label>
            <label className="text-xs text-gray-500">
              Monthly budget USD
              <input
                className="mt-1 w-full rounded border border-line bg-ink px-2 py-1.5 text-sm"
                value={budgetDollars}
                onChange={(e) => setBudgetDollars(e.target.value)}
              />
            </label>
            <label className="text-xs text-gray-500">
              Status
              <select
                className="mt-1 w-full rounded border border-line bg-ink px-2 py-1.5 text-sm"
                value={status}
                onChange={(e) => setStatus(e.target.value)}
              >
                <option value="active">active</option>
                <option value="paused">paused</option>
                <option value="terminated">terminated</option>
              </select>
            </label>
            <label className="text-xs text-gray-500">
              Sort order
              <input
                className="mt-1 w-full rounded border border-line bg-ink px-2 py-1.5 text-sm"
                value={sortOrder}
                onChange={(e) => setSortOrder(e.target.value)}
              />
            </label>
            <label className="sm:col-span-2 text-xs text-gray-500">
              Adapter JSON
              <textarea
                className="mt-1 min-h-[72px] w-full rounded border border-line bg-ink px-2 py-1.5 font-mono text-xs"
                value={adapterJson}
                onChange={(e) => setAdapterJson(e.target.value)}
              />
            </label>
          </div>
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <button
              type="button"
              disabled={saving || removing}
              className="rounded-full bg-white px-4 py-2 text-sm font-medium text-black disabled:opacity-50"
              onClick={() => void save()}
            >
              {saving ? "Saving…" : "Save changes"}
            </button>
            <button
              type="button"
              disabled={saving || removing}
              className="rounded-full border border-red-900/60 bg-red-950/40 px-4 py-2 text-sm text-red-200 hover:bg-red-950/60 disabled:opacity-50"
              onClick={() => void removeAgent()}
            >
              {removing ? "Removing…" : "Remove from roster"}
            </button>
          </div>
          <p className="mt-2 text-[11px] text-gray-600">
            <strong className="text-gray-400">Finish without deleting:</strong> set Status to{" "}
            <code className="text-gray-500">paused</code> or <code className="text-gray-500">terminated</code> — the
            row stays for audit; <code className="text-gray-500">terminated</code> hides the agent from the org chart.
            LLM checkout only resolves <code className="text-gray-500">active</code> agents.
          </p>
        </div>
      ) : null}
    </li>
  );
}
