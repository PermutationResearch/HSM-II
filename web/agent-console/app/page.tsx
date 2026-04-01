"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

function downloadJson(filename: string, data: unknown) {
  const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = filename;
  a.click();
  URL.revokeObjectURL(a.href);
}

const defaultApi = "http://127.0.0.1:3847";

type TrailResp = { lines: Record<string, unknown>[]; path: string };
type Stats = {
  home: string;
  trail_lines: number;
  memory_markdown_files: number;
  agents_enabled: number;
  tasks_in_progress: number;
  company_os?: boolean;
};

type GraphNode = { id: string; label: string; kind: string };
type GraphLink = { source: string; target: string; rel?: string };
type TrailGraphPayload = {
  source: string;
  graph: { nodes: GraphNode[]; links: GraphLink[] };
};

type SearchResp = {
  q: string;
  trail_hits: { index: number; kind?: unknown; preview?: string | null }[];
  memory_hits: { path: string; snippet: string }[];
};

type MemoryFilesResp = {
  count: number;
  files: { path: string; snippet: string }[];
};

type AutoDreamResp = Record<string, unknown>;

type NavId = "dash" | "company" | "trail" | "memory" | "graph" | "search" | "email";

type CompanyRow = {
  id: string;
  slug: string;
  display_name: string;
  hsmii_home?: string | null;
  created_at: string;
};
type CoHealth = { postgres_configured: boolean; postgres_ok: boolean };
type TaskRow = {
  id: string;
  title: string;
  state: string;
  specification?: string | null;
  goal_ancestry?: unknown;
  checked_out_by?: string | null;
  checked_out_until?: string | null;
  owner_persona?: string | null;
  due_at?: string | null;
  sla_policy?: string | null;
  escalate_after?: string | null;
  status_reason?: string | null;
  priority?: number;
  decision_mode?: "auto" | "admin_required" | "blocked" | string;
};
type GoalRowUi = {
  id: string;
  company_id?: string;
  parent_goal_id: string | null;
  title: string;
  description?: string | null;
  status: string;
};
type GovEvent = {
  id: string;
  actor: string;
  action: string;
  subject_type: string;
  subject_id: string;
  payload?: unknown;
  created_at: string;
};
type SpendSummary = { company_id: string; total_usd: number; by_kind: { kind: string; amount_usd: number }[] };
type PolicyRule = {
  id: string;
  company_id: string;
  action_type: string;
  risk_level: string;
  amount_min?: number | null;
  amount_max?: number | null;
  decision_mode: "auto" | "admin_required" | "blocked" | string;
};
type QueueView = "all" | "overdue" | "atrisk" | "waiting_admin" | "pending_approvals" | "blocked";

export default function ConsolePage() {
  const api = process.env.NEXT_PUBLIC_API_BASE ?? defaultApi;
  const [view, setView] = useState<NavId>("dash");
  const [stats, setStats] = useState<Stats | null>(null);
  const [trail, setTrail] = useState<TrailResp | null>(null);
  const [trailGraph, setTrailGraph] = useState<TrailGraphPayload | null>(null);
  const [hyperHint, setHyperHint] = useState<string | null>(null);
  const [hyperFileGraph, setHyperFileGraph] = useState<{
    path: string | null;
    graph: { nodes?: GraphNode[]; links?: GraphLink[] };
  } | null>(null);
  const [memoryFiles, setMemoryFiles] = useState<MemoryFilesResp | null>(null);
  const [searchQ, setSearchQ] = useState("");
  const [searchRes, setSearchRes] = useState<SearchResp | null>(null);
  const [autodream, setAutodream] = useState<AutoDreamResp | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [emailPaste, setEmailPaste] = useState("");
  const [emailSimple, setEmailSimple] = useState(true);
  const [emailDraft, setEmailDraft] = useState<string | null>(null);
  const [emailApiErr, setEmailApiErr] = useState<string | null>(null);
  const [emailLoading, setEmailLoading] = useState(false);

  const [coHealth, setCoHealth] = useState<CoHealth | null>(null);
  const [coCompanies, setCoCompanies] = useState<CompanyRow[]>([]);
  const [coSel, setCoSel] = useState<string | null>(null);
  const [coTasks, setCoTasks] = useState<TaskRow[]>([]);
  const [coErr, setCoErr] = useState<string | null>(null);
  const [coNewSlug, setCoNewSlug] = useState("");
  const [coNewName, setCoNewName] = useState("");
  const [coNewTaskTitle, setCoNewTaskTitle] = useState("");
  const [coNewTaskSpec, setCoNewTaskSpec] = useState("");
  const [coGoals, setCoGoals] = useState<GoalRowUi[]>([]);
  const [coGovernance, setCoGovernance] = useState<GovEvent[]>([]);
  const [coSpend, setCoSpend] = useState<SpendSummary | null>(null);
  const [coEditGoal, setCoEditGoal] = useState<string | null>(null);
  const [coEditGoalTitle, setCoEditGoalTitle] = useState("");
  const [coEditGoalStatus, setCoEditGoalStatus] = useState("");
  const [coEditGoalParent, setCoEditGoalParent] = useState("");
  const [coCheckoutAgent, setCoCheckoutAgent] = useState("agent-1");
  const [coNewGoalTitle, setCoNewGoalTitle] = useState("");
  const [coNewGoalParent, setCoNewGoalParent] = useState("");
  const [coImpSuffix, setCoImpSuffix] = useState(true);
  const [coGovActor, setCoGovActor] = useState("operator");
  const [coGovAction, setCoGovAction] = useState("note");
  const [coGovSubjT, setCoGovSubjT] = useState("company");
  const [coGovSubjId, setCoGovSubjId] = useState("");
  const [coPolicyRules, setCoPolicyRules] = useState<PolicyRule[]>([]);
  const [coPolicyAction, setCoPolicyAction] = useState("send_message");
  const [coPolicyRisk, setCoPolicyRisk] = useState("medium");
  const [coPolicyAmtMin, setCoPolicyAmtMin] = useState("");
  const [coPolicyAmtMax, setCoPolicyAmtMax] = useState("");
  const [coPolicyDecision, setCoPolicyDecision] = useState("admin_required");
  const [coEvalAmount, setCoEvalAmount] = useState("");
  const [coPolicyEvalRes, setCoPolicyEvalRes] = useState<string | null>(null);
  const [coQueueView, setCoQueueView] = useState<QueueView>("all");
  const [coQueueTasks, setCoQueueTasks] = useState<TaskRow[]>([]);
  const [coSlaDueAt, setCoSlaDueAt] = useState<Record<string, string>>({});
  const [coSlaEscAt, setCoSlaEscAt] = useState<Record<string, string>>({});
  const [coSlaPol, setCoSlaPol] = useState<Record<string, string>>({});
  const [coSlaReason, setCoSlaReason] = useState<Record<string, string>>({});
  const [coSlaPrio, setCoSlaPrio] = useState<Record<string, string>>({});
  const [coDecisionReason, setCoDecisionReason] = useState<Record<string, string>>({});
  const coImportRef = useRef<HTMLInputElement>(null);

  const load = useCallback(async () => {
    setErr(null);
    try {
      const [
        s,
        t,
        g,
        hg,
        mem,
        ad,
      ] = await Promise.all([
        fetch(`${api}/api/console/stats`).then((r) => r.json()),
        fetch(`${api}/api/console/trail?limit=120`).then((r) => r.json()),
        fetch(`${api}/api/console/graph/trail?limit=500`).then((r) => r.json()),
        fetch(`${api}/api/console/graph/hypergraph`).then((r) => r.json()),
        fetch(`${api}/api/console/memory-files`).then((r) => r.json()),
        fetch(`${api}/api/console/autodream`).then((r) => r.json()),
      ]);
      setStats(s);
      setTrail(t);
      setTrailGraph(g);
      setMemoryFiles(mem);
      setAutodream(ad);
      setHyperFileGraph(hg?.path ? { path: hg.path as string, graph: (hg.graph ?? {}) as { nodes?: GraphNode[]; links?: GraphLink[] } } : null);
      if (hg?.hint && !hg?.path) setHyperHint(hg.hint as string);
      else setHyperHint(null);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, [api]);

  const runSearch = useCallback(async () => {
    const q = searchQ.trim();
    if (!q) {
      setSearchRes(null);
      return;
    }
    setErr(null);
    try {
      const r = await fetch(
        `${api}/api/console/search?q=${encodeURIComponent(q)}&limit=60`
      ).then((x) => x.json());
      setSearchRes(r);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, [api, searchQ]);

  const runEmailDraft = useCallback(async () => {
    const text = emailPaste.trim();
    if (!text) {
      setEmailApiErr("Paste an email or thread first.");
      setEmailDraft(null);
      return;
    }
    setErr(null);
    setEmailApiErr(null);
    setEmailDraft(null);
    setEmailLoading(true);
    try {
      const r = await fetch(`${api}/api/console/email-draft`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ text, simple: emailSimple }),
      });
      const j = (await r.json()) as { ok?: boolean; draft?: string; error?: string };
      if (!r.ok) {
        setEmailApiErr(j.error ?? `HTTP ${r.status}`);
        return;
      }
      if (j.ok && typeof j.draft === "string") {
        setEmailDraft(j.draft);
      } else {
        setEmailApiErr(j.error ?? "Draft failed");
      }
    } catch (e) {
      setEmailApiErr(e instanceof Error ? e.message : String(e));
    } finally {
      setEmailLoading(false);
    }
  }, [api, emailPaste, emailSimple]);

  const loadCompanyOs = useCallback(
    async (selOverride?: string | null) => {
      setCoErr(null);
      try {
        const h = await fetch(`${api}/api/company/health`).then((r) => r.json() as Promise<CoHealth>);
        setCoHealth(h);
        if (!h.postgres_configured) {
          setCoCompanies([]);
          setCoTasks([]);
          setCoGoals([]);
          setCoGovernance([]);
          setCoSpend(null);
          setCoPolicyRules([]);
          setCoQueueTasks([]);
          return;
        }
        const list = await fetch(`${api}/api/company/companies`).then((r) => {
          if (!r.ok) throw new Error(`companies ${r.status}`);
          return r.json() as Promise<{ companies: CompanyRow[] }>;
        });
        setCoCompanies(list.companies ?? []);
        const effectiveSel = selOverride !== undefined ? selOverride : coSel;
        if (effectiveSel) {
          const cid = effectiveSel;
          const [g, t, gov, sp, pr, q] = await Promise.all([
            fetch(`${api}/api/company/companies/${cid}/goals`).then((r) => {
              if (!r.ok) throw new Error(`goals ${r.status}`);
              return r.json() as Promise<{ goals: GoalRowUi[] }>;
            }),
            fetch(`${api}/api/company/companies/${cid}/tasks`).then((r) => {
              if (!r.ok) throw new Error(`tasks ${r.status}`);
              return r.json() as Promise<{ tasks: TaskRow[] }>;
            }),
            fetch(`${api}/api/company/companies/${cid}/governance/events`).then((r) => {
              if (!r.ok) throw new Error(`gov ${r.status}`);
              return r.json() as Promise<{ events: GovEvent[] }>;
            }),
            fetch(`${api}/api/company/companies/${cid}/spend/summary`).then((r) => {
              if (!r.ok) throw new Error(`spend ${r.status}`);
              return r.json() as Promise<SpendSummary>;
            }),
            fetch(`${api}/api/company/companies/${cid}/policies/rules`).then((r) => {
              if (!r.ok) throw new Error(`policy rules ${r.status}`);
              return r.json() as Promise<{ rules: PolicyRule[] }>;
            }),
            fetch(`${api}/api/company/companies/${cid}/tasks/queue?view=${coQueueView}`).then((r) => {
              if (!r.ok) throw new Error(`queue ${r.status}`);
              return r.json() as Promise<{ tasks: TaskRow[] }>;
            }),
          ]);
          setCoGoals(g.goals ?? []);
          setCoTasks(t.tasks ?? []);
          setCoGovernance(gov.events ?? []);
          setCoSpend(sp);
          setCoPolicyRules(pr.rules ?? []);
          setCoQueueTasks(q.tasks ?? []);
        } else {
          setCoTasks([]);
          setCoGoals([]);
          setCoGovernance([]);
          setCoSpend(null);
          setCoPolicyRules([]);
          setCoQueueTasks([]);
        }
      } catch (e) {
        setCoErr(e instanceof Error ? e.message : String(e));
      }
    },
    [api, coSel, coQueueView]
  );

  useEffect(() => {
    if (coSel) setCoGovSubjId(coSel);
  }, [coSel]);

  const coGoalDepth = useMemo(() => {
    const byId = new Map(coGoals.map((g) => [g.id, g]));
    const memo = new Map<string, number>();
    const depth = (id: string, visiting: Set<string>): number => {
      if (memo.has(id)) return memo.get(id)!;
      if (visiting.has(id)) return 0;
      const g = byId.get(id);
      const pid = g?.parent_goal_id;
      if (!pid) {
        memo.set(id, 0);
        return 0;
      }
      const ps = String(pid);
      visiting.add(id);
      const d = 1 + depth(ps, visiting);
      visiting.delete(id);
      memo.set(id, d);
      return d;
    };
    for (const g of coGoals) depth(g.id, new Set());
    return memo;
  }, [coGoals]);

  const coGoalsSorted = useMemo(
    () =>
      [...coGoals].sort(
        (a, b) =>
          (coGoalDepth.get(a.id) ?? 0) - (coGoalDepth.get(b.id) ?? 0) ||
          a.title.localeCompare(b.title)
      ),
    [coGoals, coGoalDepth]
  );

  const coLatestTaskDecision = useMemo(() => {
    const out = new Map<string, GovEvent>();
    for (const ev of coGovernance) {
      if (ev.action !== "task_policy_decision" || ev.subject_type !== "task") continue;
      const sid = String(ev.subject_id || "");
      if (!sid) continue;
      if (!out.has(sid)) out.set(sid, ev);
    }
    return out;
  }, [coGovernance]);

  const loadQueueView = useCallback(
    async (viewOverride?: QueueView) => {
      if (!coSel) return;
      const v = viewOverride ?? coQueueView;
      const r = await fetch(`${api}/api/company/companies/${coSel}/tasks/queue?view=${v}`);
      const j = (await r.json()) as { tasks?: TaskRow[]; error?: string };
      if (!r.ok) throw new Error(j.error ?? `queue ${r.status}`);
      setCoQueueTasks(j.tasks ?? []);
    },
    [api, coSel, coQueueView]
  );

  useEffect(() => {
    load();
    const id = setInterval(load, 5000);
    return () => clearInterval(id);
  }, [load]);

  useEffect(() => {
    if (view !== "company") return;
    void loadCompanyOs();
    const id = setInterval(() => void loadCompanyOs(), 8000);
    return () => clearInterval(id);
  }, [view, loadCompanyOs]);

  const nav = (id: NavId, label: string) => (
    <button
      type="button"
      key={id}
      onClick={() => setView(id)}
      className={`w-full rounded px-2 py-1 text-left ${
        view === id ? "bg-white/10 text-accent" : "text-gray-400 hover:bg-white/5"
      }`}
    >
      {label}
    </button>
  );

  return (
    <div className="grid min-h-screen grid-cols-[240px_1fr_300px] gap-0 border-line">
      <aside className="border-r border-line bg-panel px-3 py-4">
        <div className="mb-6 text-sm font-semibold tracking-tight text-white">HSM Console</div>
        <nav className="space-y-1 text-sm">
          {nav("dash", "Dashboard")}
          {nav("company", "Company OS")}
          {nav("email", "Email draft")}
          {nav("trail", "Trail")}
          {nav("memory", "Memory")}
          {nav("graph", "Graph")}
          {nav("search", "Search")}
        </nav>
        <div className="mt-8 text-xs text-gray-500">
          API: <span className="font-mono text-gray-400">{api}</span>
        </div>
      </aside>

      <main className="border-r border-line bg-ink px-6 py-5">
        {err && (
          <div className="mb-4 rounded border border-red-900/50 bg-red-950/30 px-3 py-2 text-sm text-red-300">
            {err} — is <code className="font-mono">hsm_console</code> running?
          </div>
        )}
        {view === "dash" && (
          <>
            <h1 className="mb-6 text-lg font-medium text-white">Overview</h1>
            <div className="mb-8 grid grid-cols-4 gap-3">
              <Stat label="Trail events" value={stats?.trail_lines ?? "—"} hint="task_trail.jsonl" />
              <Stat label="Memory .md files" value={stats?.memory_markdown_files ?? "—"} hint="under memory/" />
              <Stat label="Agents (stub)" value={stats?.agents_enabled ?? "—"} hint="wire your counters" />
              <Stat
                label="Tasks in progress (DB)"
                value={stats?.tasks_in_progress ?? "—"}
                hint={stats?.company_os ? "Company OS PostgreSQL" : "set HSM_COMPANY_OS_DATABASE_URL"}
              />
            </div>
            {autodream && (
              <div className="mb-6 rounded border border-line bg-panel p-4 text-sm text-gray-300">
                <div className="mb-2 text-xs uppercase tracking-wide text-gray-500">autoDream / instruction staleness</div>
                <pre className="overflow-auto font-mono text-[11px] text-gray-400">
                  {JSON.stringify(autodream, null, 2)}
                </pre>
              </div>
            )}
            <div className="rounded border border-line bg-panel p-4">
              <div className="mb-2 text-xs uppercase tracking-wide text-gray-500">Recent trail (JSONL)</div>
              <pre className="max-h-[360px] overflow-auto font-mono text-[11px] leading-relaxed text-gray-300">
                {trail?.lines?.length
                  ? trail.lines.map((row) => JSON.stringify(row) + "\n").join("")
                  : "No trail yet."}
              </pre>
            </div>
          </>
        )}
        {view === "company" && (
          <>
            <h1 className="mb-2 text-lg font-medium text-white">Company OS</h1>
            <p className="mb-4 max-w-3xl text-sm text-gray-500">
              Portfolio, goals, and tasks backed by PostgreSQL. Set{" "}
              <code className="rounded bg-white/5 px-1 font-mono text-[11px]">HSM_COMPANY_OS_DATABASE_URL</code>{" "}
              and restart <code className="font-mono text-[11px]">hsm_console</code>. Migrations live in{" "}
              <code className="font-mono text-[11px]">migrations/</code>.
            </p>
            {coErr && (
              <div className="mb-4 rounded border border-amber-900/50 bg-amber-950/30 px-3 py-2 text-sm text-amber-200">
                {coErr}
              </div>
            )}
            {coHealth && (
              <div className="mb-4 text-sm text-gray-400">
                Postgres:{" "}
                <span className={coHealth.postgres_ok ? "text-emerald-400" : "text-amber-400"}>
                  {coHealth.postgres_configured
                    ? coHealth.postgres_ok
                      ? "connected"
                      : "unreachable"
                    : "not configured"}
                </span>
              </div>
            )}
            <div className="mb-6 flex flex-wrap gap-4">
              <div className="min-w-[200px] rounded border border-line bg-panel p-3">
                <div className="mb-2 text-xs uppercase text-gray-500">New company</div>
                <input
                  className="mb-2 w-full rounded border border-line bg-ink px-2 py-1 text-sm"
                  placeholder="slug"
                  value={coNewSlug}
                  onChange={(e) => setCoNewSlug(e.target.value)}
                />
                <input
                  className="mb-2 w-full rounded border border-line bg-ink px-2 py-1 text-sm"
                  placeholder="display name"
                  value={coNewName}
                  onChange={(e) => setCoNewName(e.target.value)}
                />
                <button
                  type="button"
                  className="rounded bg-accent/20 px-3 py-1 text-sm text-accent"
                  onClick={async () => {
                    setCoErr(null);
                    try {
                      const r = await fetch(`${api}/api/company/companies`, {
                        method: "POST",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({ slug: coNewSlug.trim(), display_name: coNewName.trim() }),
                      });
                      const j = await r.json();
                      if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                      setCoNewSlug("");
                      setCoNewName("");
                      await loadCompanyOs();
                    } catch (e) {
                      setCoErr(e instanceof Error ? e.message : String(e));
                    }
                  }}
                >
                  Create
                </button>
              </div>
            </div>
            <div className="mb-4">
              <div className="mb-2 text-xs uppercase text-gray-500">Companies</div>
              <div className="flex flex-wrap gap-2">
                {coCompanies.map((c) => (
                  <button
                    key={c.id}
                    type="button"
                    onClick={() => setCoSel(c.id)}
                    className={`rounded border px-3 py-1 text-sm ${
                      coSel === c.id ? "border-accent bg-accent/10 text-accent" : "border-line text-gray-300"
                    }`}
                  >
                    {c.display_name}
                  </button>
                ))}
                {!coCompanies.length && coHealth?.postgres_configured && (
                  <span className="text-sm text-gray-600">No companies yet.</span>
                )}
              </div>
            </div>
            {coSel && (
              <>
                <div className="mb-4 flex flex-wrap items-center gap-3">
                  <span className="text-xs uppercase text-gray-500">Bundle</span>
                  <button
                    type="button"
                    className="rounded border border-line bg-panel px-3 py-1 text-sm text-gray-200 hover:bg-white/5"
                    onClick={async () => {
                      setCoErr(null);
                      try {
                        const r = await fetch(`${api}/api/company/companies/${coSel}/export`);
                        const j = await r.json();
                        if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                        downloadJson(`company-export-${coSel.slice(0, 8)}.json`, j);
                      } catch (e) {
                        setCoErr(e instanceof Error ? e.message : String(e));
                      }
                    }}
                  >
                    Export JSON
                  </button>
                  <input
                    ref={coImportRef}
                    type="file"
                    accept="application/json,.json"
                    className="hidden"
                    onChange={async (e) => {
                      const file = e.target.files?.[0];
                      e.target.value = "";
                      if (!file) return;
                      setCoErr(null);
                      try {
                        const text = await file.text();
                        const bundle = JSON.parse(text) as Record<string, unknown>;
                        const r = await fetch(`${api}/api/company/import`, {
                          method: "POST",
                          headers: { "Content-Type": "application/json" },
                          body: JSON.stringify({
                            ...bundle,
                            slug_suffix_if_exists: coImpSuffix,
                          }),
                        });
                        const j = await r.json();
                        if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                        const nid = (j as { company_id?: string }).company_id;
                        if (nid) {
                          setCoSel(nid);
                          await loadCompanyOs(nid);
                        } else {
                          await loadCompanyOs();
                        }
                      } catch (e) {
                        setCoErr(e instanceof Error ? e.message : String(e));
                      }
                    }}
                  />
                  <button
                    type="button"
                    className="rounded border border-line bg-panel px-3 py-1 text-sm text-gray-200 hover:bg-white/5"
                    onClick={() => coImportRef.current?.click()}
                  >
                    Import JSON…
                  </button>
                  <label className="flex cursor-pointer items-center gap-2 text-xs text-gray-500">
                    <input
                      type="checkbox"
                      checked={coImpSuffix}
                      onChange={(e) => setCoImpSuffix(e.target.checked)}
                      className="rounded border-line"
                    />
                    Suffix slug if taken (-import)
                  </label>
                </div>

                {coSpend && (
                  <div className="mb-4 rounded border border-line bg-panel p-3">
                    <div className="mb-2 text-xs uppercase text-gray-500">Spend (LLM + other)</div>
                    <div className="text-sm text-gray-300">
                      Total USD:{" "}
                      <span className="font-mono text-accent">{coSpend.total_usd.toFixed(4)}</span>
                    </div>
                    <ul className="mt-2 text-xs text-gray-500">
                      {(coSpend.by_kind ?? []).map((row) => (
                        <li key={row.kind}>
                          {row.kind}: {row.amount_usd.toFixed(4)}
                        </li>
                      ))}
                    </ul>
                  </div>
                )}

                <div className="mb-4 rounded border border-line bg-panel p-3">
                  <div className="mb-2 text-xs uppercase text-gray-500">Agent checkout</div>
                  <div className="flex flex-wrap items-center gap-2">
                    <input
                      className="w-40 rounded border border-line bg-ink px-2 py-1 text-sm"
                      placeholder="agent_ref"
                      value={coCheckoutAgent}
                      onChange={(e) => setCoCheckoutAgent(e.target.value)}
                    />
                    <span className="text-xs text-gray-600">Used for task checkout / release actor default</span>
                  </div>
                </div>

                <div className="mb-4 grid gap-4 md:grid-cols-2">
                  <div className="rounded border border-line bg-panel p-3">
                    <div className="mb-2 text-xs uppercase text-gray-500">Policy rules</div>
                    <div className="mb-2 grid grid-cols-2 gap-2">
                      <input
                        className="rounded border border-line bg-ink px-2 py-1 text-sm"
                        placeholder="action_type"
                        value={coPolicyAction}
                        onChange={(e) => setCoPolicyAction(e.target.value)}
                      />
                      <select
                        className="rounded border border-line bg-ink px-2 py-1 text-sm"
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
                    </div>
                    <div className="mb-2 flex flex-wrap gap-2">
                      <select
                        className="rounded border border-line bg-ink px-2 py-1 text-sm"
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
                          if (!coSel) return;
                          setCoErr(null);
                          try {
                            const numOrUndef = (v: string) => {
                              const t = v.trim();
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
                          if (!coSel) return;
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
                  </div>

                  <div className="rounded border border-line bg-panel p-3">
                    <div className="mb-2 text-xs uppercase text-gray-500">Queue views</div>
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
                            <span
                              className={`rounded px-2 py-0.5 text-[10px] ${
                                (t.decision_mode ?? (t.state === "waiting_admin" ? "admin_required" : t.state === "blocked" ? "blocked" : "auto")) === "blocked"
                                  ? "bg-red-900/40 text-red-300"
                                  : (t.decision_mode ?? (t.state === "waiting_admin" ? "admin_required" : t.state === "blocked" ? "blocked" : "auto")) === "admin_required"
                                  ? "bg-amber-900/40 text-amber-300"
                                  : "bg-emerald-900/40 text-emerald-300"
                              }`}
                            >
                              {((t.decision_mode ?? (t.state === "waiting_admin" ? "admin_required" : t.state === "blocked" ? "blocked" : "auto")).toUpperCase())}
                            </span>
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
                  </div>
                </div>

                <div className="mb-4 grid gap-4 md:grid-cols-2">
                  <div className="rounded border border-line bg-panel p-3">
                    <div className="mb-2 text-xs uppercase text-gray-500">New goal</div>
                    <input
                      className="mb-2 w-full rounded border border-line bg-ink px-2 py-1 text-sm"
                      placeholder="title"
                      value={coNewGoalTitle}
                      onChange={(e) => setCoNewGoalTitle(e.target.value)}
                    />
                    <input
                      className="mb-2 w-full rounded border border-line bg-ink px-2 py-1 font-mono text-[11px]"
                      placeholder="parent goal UUID (optional)"
                      value={coNewGoalParent}
                      onChange={(e) => setCoNewGoalParent(e.target.value)}
                    />
                    <button
                      type="button"
                      className="rounded bg-accent/20 px-3 py-1 text-sm text-accent"
                      onClick={async () => {
                        setCoErr(null);
                        try {
                          const pid = coNewGoalParent.trim();
                          const r = await fetch(`${api}/api/company/companies/${coSel}/goals`, {
                            method: "POST",
                            headers: { "Content-Type": "application/json" },
                            body: JSON.stringify({
                              title: coNewGoalTitle.trim(),
                              parent_goal_id: pid || undefined,
                            }),
                          });
                          const j = await r.json();
                          if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                          setCoNewGoalTitle("");
                          setCoNewGoalParent("");
                          await loadCompanyOs();
                        } catch (e) {
                          setCoErr(e instanceof Error ? e.message : String(e));
                        }
                      }}
                    >
                      Add goal
                    </button>
                  </div>

                  <div className="rounded border border-line bg-panel p-3">
                    <div className="mb-2 text-xs uppercase text-gray-500">Log governance event</div>
                    <input
                      className="mb-2 w-full rounded border border-line bg-ink px-2 py-1 text-sm"
                      placeholder="actor"
                      value={coGovActor}
                      onChange={(e) => setCoGovActor(e.target.value)}
                    />
                    <input
                      className="mb-2 w-full rounded border border-line bg-ink px-2 py-1 text-sm"
                      placeholder="action"
                      value={coGovAction}
                      onChange={(e) => setCoGovAction(e.target.value)}
                    />
                    <input
                      className="mb-2 w-full rounded border border-line bg-ink px-2 py-1 text-sm"
                      placeholder="subject_type (e.g. company, task)"
                      value={coGovSubjT}
                      onChange={(e) => setCoGovSubjT(e.target.value)}
                    />
                    <input
                      className="mb-2 w-full rounded border border-line bg-ink px-2 py-1 font-mono text-[11px]"
                      placeholder="subject_id (UUID)"
                      value={coGovSubjId}
                      onChange={(e) => setCoGovSubjId(e.target.value)}
                    />
                    <button
                      type="button"
                      className="rounded bg-accent/20 px-3 py-1 text-sm text-accent"
                      onClick={async () => {
                        setCoErr(null);
                        try {
                          const r = await fetch(`${api}/api/company/companies/${coSel}/governance/events`, {
                            method: "POST",
                            headers: { "Content-Type": "application/json" },
                            body: JSON.stringify({
                              actor: coGovActor.trim(),
                              action: coGovAction.trim(),
                              subject_type: coGovSubjT.trim(),
                              subject_id: coGovSubjId.trim(),
                              payload: {},
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
                      Append event
                    </button>
                  </div>
                </div>

                <div className="mb-4 rounded border border-line bg-panel">
                  <div className="border-b border-line px-3 py-2 text-xs uppercase text-gray-500">
                    Goals (tree)
                  </div>
                  <ul className="divide-y divide-line">
                    {coGoalsSorted.map((g) => {
                      const depth = coGoalDepth.get(g.id) ?? 0;
                      return (
                        <li key={g.id} className="px-3 py-2 text-sm" style={{ paddingLeft: 12 + depth * 14 }}>
                          <div className="flex flex-wrap items-baseline justify-between gap-2">
                            <div>
                              <span className="font-medium text-gray-200">{g.title}</span>
                              <span className="ml-2 text-xs text-gray-500">{g.status}</span>
                            </div>
                            <button
                              type="button"
                              className="text-xs text-accent hover:underline"
                              onClick={() => {
                                setCoEditGoal(g.id);
                                setCoEditGoalTitle(g.title);
                                setCoEditGoalStatus(g.status);
                                setCoEditGoalParent(g.parent_goal_id ? String(g.parent_goal_id) : "");
                              }}
                            >
                              Edit
                            </button>
                          </div>
                          {coEditGoal === g.id && (
                            <div className="mt-2 space-y-2 rounded border border-line bg-ink/50 p-2">
                              <input
                                className="w-full rounded border border-line bg-ink px-2 py-1 text-sm"
                                value={coEditGoalTitle}
                                onChange={(e) => setCoEditGoalTitle(e.target.value)}
                              />
                              <input
                                className="w-full rounded border border-line bg-ink px-2 py-1 text-sm"
                                placeholder="status"
                                value={coEditGoalStatus}
                                onChange={(e) => setCoEditGoalStatus(e.target.value)}
                              />
                              <input
                                className="w-full rounded border border-line bg-ink px-2 py-1 font-mono text-[11px]"
                                placeholder="parent goal UUID (empty = root)"
                                value={coEditGoalParent}
                                onChange={(e) => setCoEditGoalParent(e.target.value)}
                              />
                              <div className="flex flex-wrap gap-2">
                                <button
                                  type="button"
                                  className="rounded bg-accent/20 px-2 py-1 text-xs text-accent"
                                  onClick={async () => {
                                    setCoErr(null);
                                    try {
                                      const p = coEditGoalParent.trim();
                                      const r = await fetch(
                                        `${api}/api/company/companies/${coSel}/goals/${g.id}`,
                                        {
                                          method: "PATCH",
                                          headers: { "Content-Type": "application/json" },
                                          body: JSON.stringify({
                                            title: coEditGoalTitle.trim(),
                                            status: coEditGoalStatus.trim(),
                                            parent_goal_id: p || null,
                                          }),
                                        }
                                      );
                                      const j = await r.json();
                                      if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                                      setCoEditGoal(null);
                                      await loadCompanyOs();
                                    } catch (e) {
                                      setCoErr(e instanceof Error ? e.message : String(e));
                                    }
                                  }}
                                >
                                  Save
                                </button>
                                <button
                                  type="button"
                                  className="rounded px-2 py-1 text-xs text-gray-500 hover:text-gray-300"
                                  onClick={() => setCoEditGoal(null)}
                                >
                                  Cancel
                                </button>
                              </div>
                            </div>
                          )}
                        </li>
                      );
                    })}
                    {!coGoalsSorted.length && <li className="px-3 py-4 text-gray-600">No goals.</li>}
                  </ul>
                </div>

                <div className="mb-4 rounded border border-line bg-panel">
                  <div className="border-b border-line px-3 py-2 text-xs uppercase text-gray-500">
                    Governance log
                  </div>
                  <ul className="max-h-[240px] divide-y divide-line overflow-auto">
                    {coGovernance.map((ev) => (
                      <li key={ev.id} className="px-3 py-2 font-mono text-[11px] text-gray-400">
                        <span className="text-gray-500">{ev.created_at}</span> · {ev.actor} · {ev.action}{" "}
                        · {ev.subject_type}/{ev.subject_id}
                      </li>
                    ))}
                    {!coGovernance.length && <li className="px-3 py-4 text-gray-600">No events.</li>}
                  </ul>
                </div>

                <div className="mb-4 flex flex-wrap gap-4">
                  <div className="min-w-[240px] flex-1 rounded border border-line bg-panel p-3">
                    <div className="mb-2 text-xs uppercase text-gray-500">New task</div>
                    <input
                      className="mb-2 w-full rounded border border-line bg-ink px-2 py-1 text-sm"
                      placeholder="title"
                      value={coNewTaskTitle}
                      onChange={(e) => setCoNewTaskTitle(e.target.value)}
                    />
                    <textarea
                      className="mb-2 w-full rounded border border-line bg-ink px-2 py-1 text-sm"
                      placeholder="specification / acceptance (optional)"
                      rows={3}
                      value={coNewTaskSpec}
                      onChange={(e) => setCoNewTaskSpec(e.target.value)}
                    />
                    <button
                      type="button"
                      className="rounded bg-accent/20 px-3 py-1 text-sm text-accent"
                      onClick={async () => {
                        setCoErr(null);
                        try {
                          const r = await fetch(`${api}/api/company/companies/${coSel}/tasks`, {
                            method: "POST",
                            headers: { "Content-Type": "application/json" },
                            body: JSON.stringify({
                              title: coNewTaskTitle.trim(),
                              specification: coNewTaskSpec.trim() || undefined,
                            }),
                          });
                          const j = await r.json();
                          if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                          setCoNewTaskTitle("");
                          setCoNewTaskSpec("");
                          await loadCompanyOs();
                        } catch (e) {
                          setCoErr(e instanceof Error ? e.message : String(e));
                        }
                      }}
                    >
                      Add task
                    </button>
                  </div>
                </div>
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
                              <span
                                className={`inline-block rounded px-1.5 py-0.5 ${
                                  (t.state === "blocked")
                                    ? "bg-red-900/40 text-red-300"
                                    : (t.state === "waiting_admin")
                                    ? "bg-amber-900/40 text-amber-300"
                                    : "bg-emerald-900/40 text-emerald-300"
                                }`}
                              >
                                {(t.state === "blocked" ? "BLOCKED" : t.state === "waiting_admin" ? "ADMIN_REQUIRED" : "AUTO")}
                              </span>
                              {t.owner_persona ? ` · ${t.owner_persona}` : ""}
                              {t.checked_out_by ? ` · out: ${t.checked_out_by}` : ""}
                              {t.checked_out_until
                                ? ` · until ${String(t.checked_out_until)}`
                                : ""}
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
                                    const t = v.trim();
                                    return t ? t : undefined;
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
              </>
            )}
          </>
        )}
        {view === "email" && (
          <>
            <h1 className="mb-2 text-lg font-medium text-white">Email draft</h1>
            <p className="mb-4 max-w-3xl text-sm text-gray-500">
              Paste a full message or thread (From / Subject / body). Uses your{" "}
              <code className="rounded bg-white/5 px-1 font-mono text-[11px]">HSMII_HOME</code>{" "}
              (MEMORY.md, business pack, living prompt) and Ollama. First request loads the agent and can take
              a while.
            </p>
            <label className="mb-2 flex cursor-pointer items-center gap-2 text-sm text-gray-400">
              <input
                type="checkbox"
                checked={emailSimple}
                onChange={(e) => setEmailSimple(e.target.checked)}
                className="rounded border-line"
              />
              Simple mode (one LLM call, recommended for paste). Off = tools can read{" "}
              <code className="font-mono text-xs">read_eml</code> / files (slower).
            </label>
            <textarea
              className="mb-3 min-h-[220px] w-full max-w-4xl rounded border border-line bg-panel p-3 font-mono text-sm text-gray-200 placeholder:text-gray-600"
              placeholder="Paste email here…"
              value={emailPaste}
              onChange={(e) => setEmailPaste(e.target.value)}
            />
            <div className="mb-4 flex flex-wrap items-center gap-3">
              <button
                type="button"
                disabled={emailLoading}
                className="rounded bg-accent/20 px-4 py-2 text-sm font-medium text-accent disabled:opacity-50"
                onClick={runEmailDraft}
              >
                {emailLoading ? "Drafting…" : "Draft reply"}
              </button>
              <button
                type="button"
                className="text-xs text-gray-500 hover:text-gray-400"
                onClick={() => {
                  setEmailPaste("");
                  setEmailDraft(null);
                  setEmailApiErr(null);
                }}
              >
                Clear
              </button>
            </div>
            {emailApiErr && (
              <div className="mb-4 max-w-4xl rounded border border-red-900/50 bg-red-950/30 px-3 py-2 text-sm text-red-300">
                {emailApiErr}
              </div>
            )}
            {emailDraft !== null && (
              <div className="max-w-4xl rounded border border-line bg-panel p-4">
                <div className="mb-2 text-xs uppercase tracking-wide text-gray-500">Draft (copy before sending)</div>
                <pre className="whitespace-pre-wrap font-sans text-sm leading-relaxed text-gray-200">
                  {emailDraft}
                </pre>
              </div>
            )}
          </>
        )}
        {view === "trail" && (
          <>
            <h1 className="mb-4 text-lg font-medium text-white">Trail</h1>
            <p className="mb-4 text-sm text-gray-500">{trail?.path}</p>
            <pre className="max-h-[70vh] overflow-auto rounded border border-line bg-panel p-4 font-mono text-[11px] text-gray-300">
              {trail?.lines?.length
                ? trail.lines.map((row) => JSON.stringify(row) + "\n").join("")
                : "Empty."}
            </pre>
          </>
        )}
        {view === "memory" && (
          <>
            <h1 className="mb-4 text-lg font-medium text-white">Memory files</h1>
            <p className="mb-4 text-sm text-gray-400">{memoryFiles?.count ?? 0} markdown files under memory/</p>
            <ul className="max-h-[70vh] space-y-3 overflow-auto">
              {memoryFiles?.files?.map((f) => (
                <li key={f.path} className="rounded border border-line bg-panel p-3 text-sm">
                  <div className="font-mono text-accent">{f.path}</div>
                  <div className="mt-1 text-xs text-gray-500">{f.snippet}</div>
                </li>
              ))}
            </ul>
          </>
        )}
        {view === "graph" && (
          <>
            <h1 className="mb-2 text-lg font-medium text-white">Graph</h1>
            <p className="mb-4 text-sm text-gray-500">
              Hyperedges from trail JSONL (relation hub → participants). Export file-based hypergraph via{" "}
              <code className="rounded bg-white/5 px-1 font-mono text-[11px]">viz/hyper_graph.json</code>.
            </p>
            {hyperHint && (
              <div className="mb-4 rounded border border-amber-900/40 bg-amber-950/20 px-3 py-2 text-xs text-amber-200">
                {hyperHint}
              </div>
            )}
            <div className="mb-4 text-xs text-gray-500">From task trail (<code className="font-mono">hyperedge</code> events)</div>
            <TrailGraphView graph={trailGraph?.graph} />
            {hyperFileGraph?.path &&
            ((hyperFileGraph.graph.nodes?.length ?? 0) > 0 || (hyperFileGraph.graph.links?.length ?? 0) > 0) ? (
              <>
                <div className="mt-8 mb-4 text-xs text-gray-500">
                  File export: <code className="font-mono text-gray-400">{hyperFileGraph.path}</code>
                </div>
                <TrailGraphView
                  graph={{
                    nodes: hyperFileGraph.graph.nodes ?? [],
                    links: hyperFileGraph.graph.links ?? [],
                  }}
                />
              </>
            ) : null}
          </>
        )}
        {view === "search" && (
          <>
            <h1 className="mb-4 text-lg font-medium text-white">Search</h1>
            <div className="mb-4 flex gap-2">
              <input
                className="flex-1 rounded border border-line bg-panel px-3 py-2 text-sm text-white placeholder:text-gray-600"
                placeholder="Substring search across trail JSON + memory snippets…"
                value={searchQ}
                onChange={(e) => setSearchQ(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && runSearch()}
              />
              <button
                type="button"
                className="rounded bg-accent/20 px-4 py-2 text-sm font-medium text-accent"
                onClick={runSearch}
              >
                Search
              </button>
            </div>
            {searchRes && (
              <div className="grid gap-6 md:grid-cols-2">
                <div>
                  <div className="mb-2 text-xs uppercase text-gray-500">Trail hits</div>
                  <ul className="max-h-[55vh] space-y-2 overflow-auto text-sm">
                    {searchRes.trail_hits?.length ? (
                      searchRes.trail_hits.map((h, i) => (
                        <li key={i} className="rounded border border-line bg-panel p-2 font-mono text-[10px] text-gray-400">
                          {h.preview}
                        </li>
                      ))
                    ) : (
                      <li className="text-gray-600">No matches.</li>
                    )}
                  </ul>
                </div>
                <div>
                  <div className="mb-2 text-xs uppercase text-gray-500">Memory hits</div>
                  <ul className="max-h-[55vh] space-y-2 overflow-auto text-sm">
                    {searchRes.memory_hits?.length ? (
                      searchRes.memory_hits.map((h) => (
                        <li key={h.path} className="rounded border border-line bg-panel p-2">
                          <div className="font-mono text-xs text-accent">{h.path}</div>
                          <div className="text-xs text-gray-500">{h.snippet}</div>
                        </li>
                      ))
                    ) : (
                      <li className="text-gray-600">No matches.</li>
                    )}
                  </ul>
                </div>
              </div>
            )}
          </>
        )}
      </main>

      <aside className="bg-panel px-4 py-5">
        <div className="mb-3 text-xs font-semibold uppercase tracking-wide text-gray-500">Help</div>
        <div className="space-y-3 text-sm text-gray-400">
          <p>
            Polls <code className="rounded bg-white/5 px-1 font-mono text-accent">/api/console/*</code> every 5s.
          </p>
          <p className="text-xs leading-relaxed">
            Email draft tab → <code className="font-mono text-gray-300">POST /api/console/email-draft</code>{" "}
            (needs Ollama).
          </p>
          <p className="text-xs leading-relaxed">
            Personal agent: <code className="font-mono text-gray-300">AGENTS.md</code>,{" "}
            <code className="font-mono text-gray-300">prompt.template.md</code>, MCP plugins,{" "}
            <code className="font-mono text-gray-300">HSM_AUTODREAM=1</code>.
          </p>
          <code className="mt-2 block whitespace-pre-wrap break-all rounded bg-black/40 p-2 font-mono text-[10px] text-gray-300">
            cargo run -p hyper-stigmergy --bin hsm_console -- --port 3847
          </code>
        </div>
      </aside>
    </div>
  );
}

function Stat({ label, value, hint }: { label: string; value: string | number; hint: string }) {
  return (
    <div className="rounded border border-line bg-panel px-4 py-3">
      <div className="text-2xl font-semibold text-white">{value}</div>
      <div className="text-xs text-gray-400">{label}</div>
      <div className="mt-1 text-[10px] text-gray-600">{hint}</div>
    </div>
  );
}

function TrailGraphView({ graph }: { graph?: { nodes: GraphNode[]; links: GraphLink[] } }) {
  const layout = useMemo(() => {
    const nodes = graph?.nodes ?? [];
    if (!nodes.length) return { pts: new Map<string, { x: number; y: number }>(), w: 400, h: 320 };

    const w = 520;
    const h = 420;
    const cx = w / 2;
    const cy = h / 2;
    const r = Math.min(w, h) / 2 - 40;
    const pts = new Map<string, { x: number; y: number }>();
    nodes.forEach((n, i) => {
      const ang = (2 * Math.PI * i) / nodes.length - Math.PI / 2;
      pts.set(n.id, { x: cx + r * Math.cos(ang), y: cy + r * Math.sin(ang) });
    });
    return { pts, w, h };
  }, [graph]);

  const nodes = graph?.nodes ?? [];
  const links = graph?.links ?? [];

  if (!nodes.length) {
    return (
      <div className="rounded border border-line bg-panel p-8 text-center text-sm text-gray-500">
        No hyperedge events in trail yet. Use{" "}
        <code className="font-mono text-gray-400">record_hyperedge</code> from the agent to populate.
      </div>
    );
  }

  const { pts, w, h } = layout;

  return (
    <div className="rounded border border-line bg-panel p-4">
      <svg width={w} height={h} className="mx-auto text-gray-200">
        {links.map((L, i) => {
          const a = pts.get(L.source);
          const b = pts.get(L.target);
          if (!a || !b) return null;
          return (
            <line
              key={i}
              x1={a.x}
              y1={a.y}
              x2={b.x}
              y2={b.y}
              stroke="rgba(148,163,184,0.35)"
              strokeWidth={1}
            />
          );
        })}
        {nodes.map((n) => {
          const p = pts.get(n.id);
          if (!p) return null;
          const col = n.kind === "relation" ? "#38bdf8" : "#a78bfa";
          return (
            <g key={n.id}>
              <circle cx={p.x} cy={p.y} r={n.kind === "relation" ? 8 : 5} fill={col} opacity={0.9} />
              <text x={p.x + 10} y={p.y + 4} fontSize={10} fill="#e2e8f0" className="select-none">
                {(n.label || n.id).slice(0, 32)}
              </text>
            </g>
          );
        })}
      </svg>
      <div className="mt-2 text-center text-xs text-gray-600">
        {nodes.length} nodes · {links.length} links (trail-derived)
      </div>
    </div>
  );
}
