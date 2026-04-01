"use client";

import { useCallback, useEffect, useId, useMemo, useState } from "react";

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

/** Example ids; also merged with personas seen on tasks (parent passes those). */
const EXAMPLE_AGENT_IDS = ["property_admin", "billing_clerk", "ops_lead", "concierge"] as const;

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

  const mergedIdSuggestions = useMemo(() => {
    const set = new Set<string>();
    for (const x of EXAMPLE_AGENT_IDS) set.add(x);
    for (const x of suggestedAgentIds) {
      const t = x.trim();
      if (t) set.add(t);
    }
    return [...set].sort((a, b) => a.localeCompare(b));
  }, [suggestedAgentIds]);

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
    <details className="mb-6 rounded-lg border border-line bg-panel" open>
      <summary className="cursor-pointer list-none px-4 py-3 text-sm font-medium text-gray-200 marker:content-none [&::-webkit-details-marker]:hidden">
        <span className="text-gray-400">▸</span> Workforce agents{" "}
        <span className="font-normal text-gray-500">
          (roles, org chart, adapter, budget — like Paperclip employees)
        </span>
      </summary>
      <div className="space-y-4 border-t border-line px-4 py-4">
        <p className="text-sm leading-relaxed text-gray-500">
          <strong className="text-gray-400">Agent id</strong> is the stable key: it should match a task’s{" "}
          <code className="text-xs text-accent">owner_persona</code> or the name used at checkout (letters, digits,{" "}
          <code className="text-xs">_</code>, <code className="text-xs">-</code> only).{" "}
          <strong className="text-gray-400">Role</strong> is separate — <code className="text-xs">worker</code>,{" "}
          <code className="text-xs">manager</code>, etc. describe place in the org chart, not the id.
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
                placeholder="Adapter type (e.g. ollama, claude_local)"
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
          <button
            type="button"
            disabled={saving}
            className="mt-3 rounded-full bg-white px-4 py-2 text-sm font-medium text-black disabled:opacity-50"
            onClick={() => void save()}
          >
            {saving ? "Saving…" : "Save changes"}
          </button>
        </div>
      ) : null}
    </li>
  );
}
