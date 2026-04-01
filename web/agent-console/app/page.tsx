"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { OnboardingWizard, OnboardDraft } from "./components/OnboardingWizard";
import { CompanyAgentsPanel, type CoAgentRow } from "./components/CompanyAgentsPanel";
import { PolicyQueuePanel } from "./components/PolicyQueuePanel";
import type { QueueView } from "./lib/inboxPlainLanguage";
import { TaskListPanel, type TaskListDashboardFilter } from "./components/TaskListPanel";
import { GoalGovernancePanel } from "./components/GoalGovernancePanel";
import { OrchestrationPanels } from "./components/OrchestrationPanels";
import { AntiSycophancyPanel } from "./components/AntiSycophancyPanel";
import { useCompaniesShCatalog, type CompaniesShItem } from "../ui/src/hooks/useCompaniesShCatalog";
import { WorkspaceSidebar, type WorkspaceConsoleView } from "../ui/src/components/WorkspaceSidebar";
import { Dashboard, type DashboardDrillDown } from "../ui/src/pages/Dashboard";

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

type NavId =
  | "dash"
  | "onboard"
  | "company"
  | "command"
  | "quality"
  | "trail"
  | "memory"
  | "graph"
  | "search"
  | "email";

type CompanyRow = {
  id: string;
  slug: string;
  display_name: string;
  hsmii_home?: string | null;
  issue_key_prefix?: string;
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
  parent_task_id?: string | null;
  spawned_by_rule_id?: string | null;
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

export default function ConsolePage() {
  const api = process.env.NEXT_PUBLIC_API_BASE ?? defaultApi;
  const [view, setView] = useState<NavId>("command");
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
  const [focusAgentPersona, setFocusAgentPersona] = useState<string | null>(null);
  const [taskDashboardFilter, setTaskDashboardFilter] = useState<TaskListDashboardFilter | null>(null);
  const [dashScrollTaskId, setDashScrollTaskId] = useState<string | null>(null);
  const [coSpendOpen, setCoSpendOpen] = useState(false);
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
  const [coAgents, setCoAgents] = useState<CoAgentRow[]>([]);
  const [coQueueView, setCoQueueView] = useState<QueueView>("all");
  const [coQueueTasks, setCoQueueTasks] = useState<TaskRow[]>([]);
  /** Splits Company OS into daily work vs team setup vs power tools. */
  const [coWorkspaceTab, setCoWorkspaceTab] = useState<"work" | "team" | "advanced">("work");
  const [coSlaDueAt, setCoSlaDueAt] = useState<Record<string, string>>({});
  const [coSlaEscAt, setCoSlaEscAt] = useState<Record<string, string>>({});
  const [coSlaPol, setCoSlaPol] = useState<Record<string, string>>({});
  const [coSlaReason, setCoSlaReason] = useState<Record<string, string>>({});
  const [coSlaPrio, setCoSlaPrio] = useState<Record<string, string>>({});
  const [coDecisionReason, setCoDecisionReason] = useState<Record<string, string>>({});
  const [obVertical, setObVertical] = useState("generic_smb");
  const [obInput, setObInput] = useState("");
  const [obTranscript, setObTranscript] = useState<string[]>([]);
  const [obLoading, setObLoading] = useState(false);
  const [obDraft, setObDraft] = useState<OnboardDraft | null>(null);
  const [obApplyLoading, setObApplyLoading] = useState(false);
  const [obApplyMsg, setObApplyMsg] = useState<string | null>(null);
  const coImportRef = useRef<HTMLInputElement>(null);
  const companiesSh = useCompaniesShCatalog();

  const DASH_LAYOUT_STORAGE = "hsm-dashboard-layout";
  /** Default Paperclip-style dense console; user can switch to "Overview" in the toggle. */
  const [commandDashboardLayout, setCommandDashboardLayout] = useState<"nothing" | "admin">("admin");

  useEffect(() => {
    try {
      const raw = localStorage.getItem(DASH_LAYOUT_STORAGE);
      if (raw === "admin" || raw === "nothing") setCommandDashboardLayout(raw);
    } catch {
      /* private mode */
    }
  }, []);

  const persistCommandDashboardLayout = useCallback((next: "nothing" | "admin") => {
    setCommandDashboardLayout(next);
    try {
      localStorage.setItem(DASH_LAYOUT_STORAGE, next);
    } catch {
      /* ignore */
    }
  }, []);

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
          setCoAgents([]);
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
          const [g, t, ag, gov, sp, pr, q] = await Promise.all([
            fetch(`${api}/api/company/companies/${cid}/goals`).then((r) => {
              if (!r.ok) throw new Error(`goals ${r.status}`);
              return r.json() as Promise<{ goals: GoalRowUi[] }>;
            }),
            fetch(`${api}/api/company/companies/${cid}/tasks`).then((r) => {
              if (!r.ok) throw new Error(`tasks ${r.status}`);
              return r.json() as Promise<{ tasks: TaskRow[] }>;
            }),
            fetch(`${api}/api/company/companies/${cid}/agents`).then((r) => {
              if (!r.ok) throw new Error(`agents ${r.status}`);
              return r.json() as Promise<{ agents: CoAgentRow[] }>;
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
          setCoAgents(ag.agents ?? []);
          setCoGovernance(gov.events ?? []);
          setCoSpend(sp);
          setCoPolicyRules(pr.rules ?? []);
          setCoQueueTasks(q.tasks ?? []);
        } else {
          setCoTasks([]);
          setCoGoals([]);
          setCoAgents([]);
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

  const obNextQuestion = useMemo(() => {
    if (obDraft?.missing_critical_items?.length) {
      const miss = obDraft.missing_critical_items[0];
      if (miss === "company_name") return "What is your official company name?";
      if (miss === "approver_role") return "Who should approve sensitive actions (refunds, legal, budget)?";
      if (miss === "workflows") return "What are your top 3 recurring workflows?";
      return `Please clarify: ${miss}`;
    }
    // Adaptive follow-ups even when critical fields are present
    if (!obTranscript.some((x) => /urgent|1h|same day|24h/i.test(x))) {
      return "Which requests are urgent (1h), same day, or can wait 24h?";
    }
    if (!obTranscript.some((x) => /refund|budget|legal|approve|manager|owner/i.test(x))) {
      return "Which actions should AI ask approval for (refunds, legal replies, budget edits)?";
    }
    if (!obTranscript.some((x) => /email|crm|helpdesk|shopify|ads|accounting/i.test(x))) {
      return "Which tools do you use now (email, CRM, helpdesk, ecommerce, ads, accounting)?";
    }
    return "Anything else AI should never do automatically?";
  }, [obDraft, obTranscript]);

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

  const clearDashScrollTask = useCallback(() => setDashScrollTaskId(null), []);

  const handleDashboardDrill = useCallback(
    async (d: DashboardDrillDown) => {
      setView("company");
      setTaskDashboardFilter(null);
      setDashScrollTaskId(null);
      setFocusAgentPersona(null);
      setCoSpendOpen(false);
      setCoErr(null);
      setCoWorkspaceTab(d.type === "spend" ? "advanced" : "work");

      if (!coSel) return;

      const q = async (v: QueueView) => {
        setCoQueueView(v);
        await loadQueueView(v);
      };

      switch (d.type) {
        case "inbox":
          await q("all");
          return;
        case "queue":
          await q(d.view as QueueView);
          return;
        case "task":
          await q("all");
          setDashScrollTaskId(d.taskId);
          return;
        case "persona":
          setFocusAgentPersona(d.persona);
          await q("all");
          return;
        case "filter_priority":
          setTaskDashboardFilter({ kind: "priority", level: d.level });
          await q("all");
          return;
        case "filter_state":
          setTaskDashboardFilter({ kind: "state", state: d.state });
          await q("all");
          return;
        case "filter_task_ids":
          setTaskDashboardFilter({ kind: "ids", ids: d.ids });
          await q("all");
          return;
        case "filter_in_progress":
          setTaskDashboardFilter({ kind: "in_progress" });
          await q("all");
          return;
        case "filter_open":
          setTaskDashboardFilter({ kind: "open" });
          await q("all");
          return;
        case "filter_blocked":
          setTaskDashboardFilter({ kind: "blocked" });
          await q("all");
          return;
        case "filter_completed":
          setTaskDashboardFilter({ kind: "completed" });
          await q("all");
          return;
        case "spend":
          setCoSpendOpen(true);
          await q("all");
          return;
        default:
          return;
      }
    },
    [coSel, loadQueueView]
  );

  const createFromCatalog = useCallback(
    async (item: CompaniesShItem) => {
      if (coHealth && !coHealth.postgres_configured) {
        setCoErr("Set HSM_COMPANY_OS_DATABASE_URL and restart hsm_console to add companies.");
        return;
      }
      setCoErr(null);
      let base = item.slug
        .trim()
        .toLowerCase()
        .replace(/[^a-z0-9_-]+/g, "-")
        .replace(/-+/g, "-")
        .replace(/^-|-$/g, "");
      if (!base) base = "company";
      const display_name = item.name.trim() || base;
      let slug = base;
      for (let i = 0; i < 8; i++) {
        const r = await fetch(`${api}/api/company/companies`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ slug, display_name, hsmii_home: null }),
        });
        const j = (await r.json()) as { company?: { id: string }; error?: string };
        if (r.ok && j.company?.id) {
          setCoSel(j.company.id);
          await loadCompanyOs(j.company.id);
          return;
        }
        if (r.status === 409) {
          slug = `${base}-${i + 2}`;
          continue;
        }
        setCoErr(j.error ?? `HTTP ${r.status}`);
        return;
      }
      setCoErr("Could not create company (slug conflict).");
    },
    [api, coHealth, loadCompanyOs]
  );

  useEffect(() => {
    load();
    const id = setInterval(load, 5000);
    return () => clearInterval(id);
  }, [load]);

  useEffect(() => {
    void loadCompanyOs();
    const id = setInterval(() => void loadCompanyOs(), 8000);
    return () => clearInterval(id);
  }, [loadCompanyOs]);

  useEffect(() => {
    if (coSel || coCompanies.length === 0) return;
    setCoSel(coCompanies[0].id);
  }, [coSel, coCompanies]);

  useEffect(() => {
    setFocusAgentPersona(null);
    setTaskDashboardFilter(null);
    setDashScrollTaskId(null);
    setCoSpendOpen(false);
    setCoWorkspaceTab("work");
  }, [coSel]);

  const workspaceLabel = coCompanies.find((c) => c.id === coSel)?.display_name ?? "Workspace";
  const workspaceInitial = workspaceLabel.replace(/\s+/g, "").slice(0, 1) || "W";

  const sidebarAgents = useMemo(() => {
    const m = new Map<string, { id: string; name: string; liveCount: number }>();
    for (const a of coAgents) {
      if (a.status === "terminated") continue;
      m.set(a.name, { id: a.name, name: a.name, liveCount: 0 });
    }
    for (const t of coTasks) {
      const id = (t.owner_persona ?? t.checked_out_by ?? "").trim();
      if (!id) continue;
      if (!m.has(id)) m.set(id, { id, name: id, liveCount: 0 });
      const row = m.get(id)!;
      if (t.checked_out_by || /progress|doing|active/i.test(t.state)) row.liveCount += 1;
    }
    return Array.from(m.values());
  }, [coTasks, coAgents]);

  const dashboardLiveCount = useMemo(
    () => coTasks.filter((t) => t.checked_out_by || /progress|doing|active/i.test(t.state)).length,
    [coTasks]
  );

  const inboxCount = useMemo(() => {
    if (coQueueTasks.length > 0) return coQueueTasks.length;
    return coTasks.filter((t) => /open|todo|pending/i.test(t.state)).length;
  }, [coQueueTasks, coTasks]);

  /** Personas / checkout names on tasks → agent id suggestions in Team tab. */
  const coAgentIdSuggestions = useMemo(() => {
    const s = new Set<string>();
    for (const t of coTasks) {
      const o = (t.owner_persona ?? "").trim();
      if (o) s.add(o);
      const c = (t.checked_out_by ?? "").trim();
      if (c) s.add(c);
    }
    return [...s];
  }, [coTasks]);

  const sidebarProjects = useMemo(
    () => coGoalsSorted.map((g) => ({ id: g.id, name: g.title })),
    [coGoalsSorted]
  );

  const focusAgentGovernance = useMemo(() => {
    const p = focusAgentPersona?.trim();
    if (!p) return [];
    return coGovernance
      .filter((e) => (e.actor ?? "").trim() === p)
      .slice()
      .sort((a, b) => String(b.created_at).localeCompare(String(a.created_at)))
      .slice(0, 12);
  }, [coGovernance, focusAgentPersona]);

  /** Sidebar agent click sets persona id — match workforce row for friendlier labels. */
  const focusAgentMeta = useMemo(() => {
    const id = focusAgentPersona?.trim();
    if (!id) return null;
    const agent = coAgents.find((a) => a.name === id);
    return {
      id,
      role: agent?.role?.trim() || null,
      title: agent?.title?.trim() || null,
      inRegistry: !!agent,
    };
  }, [focusAgentPersona, coAgents]);

  return (
    <div className="flex min-h-screen border-line bg-black">
      <WorkspaceSidebar
        workspaceLabel={workspaceLabel}
        workspaceInitial={workspaceInitial}
        companies={coCompanies.map((c) => ({ id: c.id, display_name: c.display_name }))}
        selectedCompanyId={coSel}
        onSelectCompany={(id) => {
          setCoSel(id);
          void loadCompanyOs(id);
        }}
        view={view as WorkspaceConsoleView}
        onNavigate={(id) => setView(id as NavId)}
        dashboardLiveCount={dashboardLiveCount}
        inboxCount={inboxCount}
        projects={sidebarProjects}
        agents={sidebarAgents}
        selectedAgentPersona={focusAgentPersona}
        onSelectAgent={(persona) => {
          setFocusAgentPersona(persona);
          setView("company");
          setCoWorkspaceTab("work");
        }}
        onNewIssue={() => {
          setView("company");
          setCoWorkspaceTab("work");
        }}
        onOpenOnboarding={() => setView("onboard")}
        apiBase={api}
        catalog={{
          items: companiesSh.items,
          loading: companiesSh.loading,
          error: companiesSh.error,
        }}
        onCreateFromCatalog={createFromCatalog}
      />

      <main
        className={
          view === "command" || view === "company"
            ? "min-h-screen min-w-0 flex-1 overflow-auto bg-[#010409] px-6 py-5"
            : "min-h-screen min-w-0 flex-1 overflow-auto px-6 py-5"
        }
      >
        {err && (
          <div className="mb-4 rounded-2xl border border-[#D71921] bg-card px-3 py-2 text-sm text-[#E8E8E8]">
            <span className="font-mono text-[11px] uppercase tracking-wide text-[#D71921]">[ERROR]</span> {err} — is{" "}
            <code className="font-mono text-[#999999]">hsm_console</code> running?
          </div>
        )}
        {view === "dash" && (
          <>
            <h1 className="mb-6 text-lg font-medium text-white">System overview</h1>
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
        {view === "command" && (
          <div>
            {coErr && (
              <div className="mb-4 rounded border border-amber-900/50 bg-amber-950/30 px-3 py-2 text-sm text-amber-200">
                {coErr}
              </div>
            )}
            <div className="mb-4 flex flex-wrap items-center justify-end gap-2 border-b border-[#222222] pb-4">
              <span className="mr-auto font-mono text-[10px] uppercase tracking-[0.08em] text-[#666666]">
                Dashboard view
              </span>
              <div className="inline-flex rounded-full border border-[#333333] p-0.5">
                <button
                  type="button"
                  onClick={() => persistCommandDashboardLayout("nothing")}
                  className={
                    commandDashboardLayout === "nothing"
                      ? "rounded-full bg-white px-3 py-1.5 font-mono text-[11px] font-medium uppercase tracking-wide text-black"
                      : "rounded-full px-3 py-1.5 font-mono text-[11px] font-medium uppercase tracking-wide text-[#999999] transition-colors hover:text-white"
                  }
                >
                  Overview
                </button>
                <button
                  type="button"
                  onClick={() => persistCommandDashboardLayout("admin")}
                  className={
                    commandDashboardLayout === "admin"
                      ? "rounded-full bg-white px-3 py-1.5 font-mono text-[11px] font-medium uppercase tracking-wide text-black"
                      : "rounded-full px-3 py-1.5 font-mono text-[11px] font-medium uppercase tracking-wide text-[#999999] transition-colors hover:text-white"
                  }
                >
                  Admin console
                </button>
              </div>
            </div>
            <Dashboard
              apiBase={api}
              companyId={coSel}
              companies={coCompanies.map((c) => ({
                id: c.id,
                display_name: c.display_name,
                issue_key_prefix: c.issue_key_prefix,
              }))}
              layout={commandDashboardLayout}
              onOpenOnboarding={() => setView("onboard")}
              onDrillDown={handleDashboardDrill}
            />
          </div>
        )}
        {view === "quality" && <AntiSycophancyPanel api={api} setErr={setErr} />}
        {view === "onboard" && (
          <OnboardingWizard
            api={api}
            obVertical={obVertical}
            setObVertical={setObVertical}
            obInput={obInput}
            setObInput={setObInput}
            obTranscript={obTranscript}
            setObTranscript={setObTranscript}
            obLoading={obLoading}
            setObLoading={setObLoading}
            obDraft={obDraft}
            setObDraft={setObDraft}
            obApplyLoading={obApplyLoading}
            setObApplyLoading={setObApplyLoading}
            obApplyMsg={obApplyMsg}
            setObApplyMsg={setObApplyMsg}
            obNextQuestion={obNextQuestion}
            setErr={setErr}
            onApplySuccess={async (cid) => {
              setCoSel(cid);
              setView("company");
              await loadCompanyOs(cid);
            }}
          />
        )}
        {view === "company" && (
          <>
            <header className="mb-6 border-b border-[#30363D] pb-5">
              <p className="font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-[#8B949E]">Inbox</p>
              <h1 className="mt-1 text-lg font-medium tracking-tight text-white">Tasks &amp; queue</h1>
              <p className="mt-2 max-w-2xl text-sm leading-relaxed text-[#8B949E]">
                Triage, approvals, and the full task list—same layout language as Command dashboard.
              </p>
            </header>
            {coSel ? (
              <div className="mb-4 max-w-3xl">
                <div className="inline-flex flex-wrap rounded-full border border-line bg-black/40 p-0.5">
                  {(
                    [
                      ["work", "Tasks & queue"],
                      ["team", "Team & roles"],
                      ["advanced", "Advanced"],
                    ] as const
                  ).map(([id, label]) => (
                    <button
                      key={id}
                      type="button"
                      onClick={() => setCoWorkspaceTab(id)}
                      className={
                        coWorkspaceTab === id
                          ? "rounded-full bg-white px-3 py-1.5 text-sm font-medium text-black"
                          : "rounded-full px-3 py-1.5 text-sm text-gray-400 transition-colors hover:text-white"
                      }
                    >
                      {label}
                    </button>
                  ))}
                </div>
                <p className="mt-2 text-xs leading-relaxed text-gray-600">
                  <strong className="font-medium text-gray-500">Tasks &amp; queue</strong> — create work, run the inbox,
                  assign and close tasks.{" "}
                  <strong className="font-medium text-gray-500">Team &amp; roles</strong> — who does what (personas,
                  briefings). <strong className="font-medium text-gray-500">Advanced</strong> — backup, spend, goals,
                  orchestration.
                </p>
              </div>
            ) : null}
            <details className="mb-4 max-w-2xl text-sm text-gray-500">
              <summary className="cursor-pointer select-none text-gray-500 hover:text-gray-400">
                Technical setup (database)
              </summary>
              <p className="mt-2 leading-relaxed">
                Company data lives in PostgreSQL. Set{" "}
                <code className="rounded bg-white/5 px-1 font-mono text-[11px]">HSM_COMPANY_OS_DATABASE_URL</code>{" "}
                and restart <code className="font-mono text-[11px]">hsm_console</code>. Migrations:{" "}
                <code className="font-mono text-[11px]">migrations/</code>.
              </p>
            </details>
            {coErr && (
              <div className="mb-4 rounded border border-amber-900/50 bg-amber-950/30 px-3 py-2 text-sm text-amber-200">
                {coErr}
              </div>
            )}
            {coHealth && !coHealth.postgres_configured && (
              <div className="mb-4 text-sm text-amber-300/90">
                Database not connected—add a workspace once Postgres is configured (see Technical setup above).
              </div>
            )}
            {coHealth?.postgres_configured && !coHealth.postgres_ok && (
              <div className="mb-4 text-sm text-amber-300/90">Database unreachable—check your server.</div>
            )}
            {coHealth?.postgres_configured && coCompanies.length === 0 && (
              <div className="mb-6 rounded-xl border border-line bg-panel p-4">
                <div className="mb-1 text-sm font-medium text-white">Create your first workspace</div>
                <p className="mb-3 text-sm text-gray-500">One workspace per business or brand you run.</p>
                <input
                  className="mb-2 w-full max-w-md rounded-lg border border-line bg-ink px-3 py-2 text-sm"
                  placeholder="Short ID (e.g. velora)"
                  value={coNewSlug}
                  onChange={(e) => setCoNewSlug(e.target.value)}
                />
                <input
                  className="mb-3 w-full max-w-md rounded-lg border border-line bg-ink px-3 py-2 text-sm"
                  placeholder="Name people see (e.g. Gestion Velora)"
                  value={coNewName}
                  onChange={(e) => setCoNewName(e.target.value)}
                />
                <button
                  type="button"
                  className="rounded-full bg-white px-4 py-2 text-sm font-medium text-black hover:bg-gray-200"
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
                  Create workspace
                </button>
              </div>
            )}
            {coCompanies.length > 0 && (
              <div className="mb-6">
                <div className="mb-2 flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
                  <div>
                    <div className="mb-2 text-sm font-medium text-gray-300">Workspace</div>
                    <div className="flex flex-wrap gap-2">
                      {coCompanies.map((c) => (
                        <button
                          key={c.id}
                          type="button"
                          onClick={() => setCoSel(c.id)}
                          className={`rounded-full border px-4 py-2 text-sm font-medium transition-colors ${
                            coSel === c.id
                              ? "border-white bg-white text-black"
                              : "border-line text-gray-300 hover:border-gray-500"
                          }`}
                        >
                          {c.display_name}
                        </button>
                      ))}
                    </div>
                  </div>
                  <div className="flex flex-wrap items-center gap-2">
                    <label className="text-xs text-gray-500" htmlFor="co-checkout-agent">
                      Sign approvals as
                    </label>
                    <input
                      id="co-checkout-agent"
                      className="w-44 rounded-lg border border-line bg-ink px-3 py-2 text-sm"
                      placeholder="e.g. your name"
                      value={coCheckoutAgent}
                      onChange={(e) => setCoCheckoutAgent(e.target.value)}
                    />
                  </div>
                </div>
                <details className="rounded-lg border border-dashed border-line/80 bg-black/20">
                  <summary className="cursor-pointer px-3 py-2 text-xs text-gray-500 hover:text-gray-400">
                    Add another workspace
                  </summary>
                  <div className="border-t border-line/60 px-3 py-3">
                    <input
                      className="mb-2 mr-2 w-40 rounded border border-line bg-ink px-2 py-1 text-sm"
                      placeholder="Short ID"
                      value={coNewSlug}
                      onChange={(e) => setCoNewSlug(e.target.value)}
                    />
                    <input
                      className="mb-2 mr-2 w-48 rounded border border-line bg-ink px-2 py-1 text-sm"
                      placeholder="Display name"
                      value={coNewName}
                      onChange={(e) => setCoNewName(e.target.value)}
                    />
                    <button
                      type="button"
                      className="rounded-full bg-accent/20 px-3 py-1 text-sm text-accent"
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
                      Add
                    </button>
                  </div>
                </details>
              </div>
            )}
            {coSel && (
              <>
                {coWorkspaceTab === "advanced" ? (
                  <>
                    <details className="mb-4 rounded-lg border border-dashed border-line/80 bg-black/20">
                      <summary className="cursor-pointer px-3 py-2 text-xs text-gray-500 hover:text-gray-400">
                        Backup, import &amp; export
                      </summary>
                      <div className="flex flex-wrap items-center gap-3 border-t border-line/60 px-3 py-3">
                        <button
                          type="button"
                          className="rounded-full border border-line bg-panel px-3 py-1.5 text-sm text-gray-200 hover:bg-white/5"
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
                          className="rounded-full border border-line bg-panel px-3 py-1.5 text-sm text-gray-200 hover:bg-white/5"
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
                    </details>

                    {coSpend ? (
                      <details
                        className="mb-4 rounded-lg border border-dashed border-line/80 bg-black/20"
                        open={coSpendOpen}
                        onToggle={(e) => setCoSpendOpen(e.currentTarget.open)}
                      >
                        <summary className="cursor-pointer px-3 py-2 text-xs text-gray-500 hover:text-gray-400">
                          AI usage &amp; spend
                        </summary>
                        <div className="border-t border-line/60 px-3 py-3">
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
                      </details>
                    ) : null}

                    <details className="mb-6 rounded-lg border border-line bg-panel">
                      <summary className="cursor-pointer list-none px-4 py-3 text-sm font-medium text-gray-200 marker:content-none [&::-webkit-details-marker]:hidden">
                        <span className="text-gray-400">▸</span> Goals &amp; governance log{" "}
                        <span className="font-normal text-gray-500">(optional)</span>
                      </summary>
                      <div className="border-t border-line px-2 pb-4 pt-2">
                        <GoalGovernancePanel
                          api={api}
                          coSel={coSel}
                          coErrSetter={setCoErr}
                          loadCompanyOs={async () => {
                            await loadCompanyOs();
                          }}
                          coNewGoalTitle={coNewGoalTitle}
                          setCoNewGoalTitle={setCoNewGoalTitle}
                          coNewGoalParent={coNewGoalParent}
                          setCoNewGoalParent={setCoNewGoalParent}
                          coGovActor={coGovActor}
                          setCoGovActor={setCoGovActor}
                          coGovAction={coGovAction}
                          setCoGovAction={setCoGovAction}
                          coGovSubjT={coGovSubjT}
                          setCoGovSubjT={setCoGovSubjT}
                          coGovSubjId={coGovSubjId}
                          setCoGovSubjId={setCoGovSubjId}
                          coGoalsSorted={coGoalsSorted}
                          coGoalDepth={coGoalDepth}
                          coEditGoal={coEditGoal}
                          setCoEditGoal={setCoEditGoal}
                          coEditGoalTitle={coEditGoalTitle}
                          setCoEditGoalTitle={setCoEditGoalTitle}
                          coEditGoalStatus={coEditGoalStatus}
                          setCoEditGoalStatus={setCoEditGoalStatus}
                          coEditGoalParent={coEditGoalParent}
                          setCoEditGoalParent={setCoEditGoalParent}
                          coGovernance={coGovernance}
                        />
                      </div>
                    </details>

                    <details className="mb-6 rounded-lg border border-line bg-panel">
                      <summary className="cursor-pointer list-none px-4 py-3 text-sm font-medium text-gray-200 marker:content-none [&::-webkit-details-marker]:hidden">
                        <span className="text-gray-400">▸</span> Orchestration &amp; scale tools{" "}
                        <span className="font-normal text-gray-500">(spawn rules, handoffs, contracts…)</span>
                      </summary>
                      <div className="border-t border-line px-2 pb-4 pt-2">
                        <OrchestrationPanels
                          api={api}
                          companyId={coSel}
                          tasks={coTasks}
                          setCoErr={setCoErr}
                          loadCompanyOs={async () => {
                            await loadCompanyOs();
                          }}
                        />
                      </div>
                    </details>
                  </>
                ) : null}

                {coWorkspaceTab === "team" ? (
                  <CompanyAgentsPanel
                    api={api}
                    companyId={coSel}
                    agents={coAgents}
                    suggestedAgentIds={coAgentIdSuggestions}
                    setCoErr={setCoErr}
                    loadCompanyOs={async () => {
                      await loadCompanyOs();
                    }}
                  />
                ) : null}

                {coWorkspaceTab === "work" ? (
                  <>
                    <div className="mb-4 flex flex-wrap gap-4">
                      <div className="min-w-[240px] flex-1 rounded border border-line bg-panel p-4">
                        <div className="mb-1 text-sm font-semibold text-white">New task</div>
                        <p className="mb-3 text-xs text-gray-500">
                          A single thing someone should do—or that you want help with.
                        </p>
                        <input
                          className="mb-2 w-full rounded-lg border border-line bg-ink px-3 py-2 text-sm"
                          placeholder="What needs to happen?"
                          value={coNewTaskTitle}
                          onChange={(e) => setCoNewTaskTitle(e.target.value)}
                        />
                        <textarea
                          className="mb-3 w-full rounded-lg border border-line bg-ink px-3 py-2 text-sm"
                          placeholder="Details or acceptance criteria (optional)"
                          rows={3}
                          value={coNewTaskSpec}
                          onChange={(e) => setCoNewTaskSpec(e.target.value)}
                        />
                        <button
                          type="button"
                          className="rounded-full bg-white px-4 py-2 text-sm font-medium text-black hover:bg-gray-200"
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
                          Add to list
                        </button>
                      </div>
                    </div>

                    {focusAgentMeta ? (
                      <div className="mb-4 rounded-2xl border border-[#30363D] bg-[#0d1117] px-4 py-4">
                        <div className="flex flex-wrap items-start justify-between gap-3">
                          <div className="min-w-0">
                            <p className="font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-[#8B949E]">
                              Filtered inbox
                            </p>
                            <h2 className="mt-1 break-all font-mono text-base font-semibold text-[#58a6ff]">
                              {focusAgentMeta.id}
                            </h2>
                            <p className="mt-2 max-w-2xl text-sm leading-relaxed text-[#8B949E]">
                              This is the <strong className="font-medium text-[#c9d1d9]">persona id</strong> stored on
                              tasks as <strong className="font-medium text-[#c9d1d9]">owner</strong> or{" "}
                              <strong className="font-medium text-[#c9d1d9]">checked out by</strong>. The queue and task
                              list below only show work tied to this id. You applied the filter by clicking this agent
                              in the left sidebar.
                            </p>
                            {focusAgentMeta.inRegistry ? (
                              <p className="mt-2 text-xs text-[#484f58]">
                                Workforce registry:{" "}
                                {focusAgentMeta.role ? (
                                  <span className="text-[#8B949E]">role “{focusAgentMeta.role}”</span>
                                ) : (
                                  <span className="text-[#8B949E]">(no role text)</span>
                                )}
                                {focusAgentMeta.title ? (
                                  <>
                                    {" "}
                                    · <span className="text-[#8B949E]">{focusAgentMeta.title}</span>
                                  </>
                                ) : null}
                              </p>
                            ) : (
                              <p className="mt-2 text-xs text-[#484f58]">
                                Not in <strong className="text-[#6e7681]">Team &amp; roles</strong> yet—only appearing
                                because it is on tasks. Add a matching row there if you want a proper profile.
                              </p>
                            )}
                          </div>
                          <button
                            type="button"
                            className="shrink-0 rounded-md border border-[#30363D] px-3 py-2 font-mono text-[11px] font-medium uppercase tracking-wide text-[#c9d1d9] hover:border-[#484f58] hover:bg-[#161b22]"
                            onClick={() => setFocusAgentPersona(null)}
                          >
                            Show all tasks
                          </button>
                        </div>
                        <div className="mt-4 border-t border-[#30363D] pt-4">
                          <p className="font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-[#8B949E]">
                            Governance log · actor = this id
                          </p>
                          <p className="mt-1 max-w-2xl text-xs leading-relaxed text-[#484f58]">
                            Only events where the <strong className="text-[#6e7681]">Actor</strong> field equals{" "}
                            <span className="font-mono text-[#8B949E]">{focusAgentMeta.id}</span> (e.g. checkout as that
                            identity). That is separate from “tasks they own,” so this section is often empty until
                            someone acts under this id.
                          </p>
                          {focusAgentGovernance.length > 0 ? (
                            <ul className="mt-3 max-h-40 space-y-1.5 overflow-auto font-mono text-[11px] text-[#8B949E]">
                              {focusAgentGovernance.map((ev) => (
                                <li key={ev.id} className="flex flex-wrap gap-x-2">
                                  <span className="text-[#484f58]">{ev.created_at}</span>
                                  <span className="text-[#c9d1d9]">{ev.action}</span>
                                  <span className="text-[#484f58]">
                                    {ev.subject_type} {ev.subject_id?.slice(0, 8)}…
                                  </span>
                                </li>
                              ))}
                            </ul>
                          ) : (
                            <p className="mt-3 text-xs text-[#484f58]">None yet for this actor string.</p>
                          )}
                        </div>
                      </div>
                    ) : null}

                    <PolicyQueuePanel
                      api={api}
                      coSel={coSel}
                      coPolicyAction={coPolicyAction}
                      setCoPolicyAction={setCoPolicyAction}
                      coPolicyRisk={coPolicyRisk}
                      setCoPolicyRisk={setCoPolicyRisk}
                      coPolicyAmtMin={coPolicyAmtMin}
                      setCoPolicyAmtMin={setCoPolicyAmtMin}
                      coPolicyAmtMax={coPolicyAmtMax}
                      setCoPolicyAmtMax={setCoPolicyAmtMax}
                      coPolicyDecision={coPolicyDecision}
                      setCoPolicyDecision={setCoPolicyDecision}
                      coEvalAmount={coEvalAmount}
                      setCoEvalAmount={setCoEvalAmount}
                      coPolicyEvalRes={coPolicyEvalRes}
                      setCoPolicyEvalRes={setCoPolicyEvalRes}
                      coPolicyRules={coPolicyRules}
                      coQueueView={coQueueView}
                      setCoQueueView={setCoQueueView}
                      coQueueTasks={coQueueTasks}
                      coDecisionReason={coDecisionReason}
                      setCoDecisionReason={setCoDecisionReason}
                      coCheckoutAgent={coCheckoutAgent}
                      setCoErr={setCoErr}
                      loadCompanyOs={async () => {
                        await loadCompanyOs();
                      }}
                      loadQueueView={loadQueueView}
                    />

                    <TaskListPanel
                      api={api}
                      coTasks={coTasks}
                      coCheckoutAgent={coCheckoutAgent}
                      coLatestTaskDecision={coLatestTaskDecision}
                      coSlaDueAt={coSlaDueAt}
                      setCoSlaDueAt={setCoSlaDueAt}
                      coSlaEscAt={coSlaEscAt}
                      setCoSlaEscAt={setCoSlaEscAt}
                      coSlaPol={coSlaPol}
                      setCoSlaPol={setCoSlaPol}
                      coSlaPrio={coSlaPrio}
                      setCoSlaPrio={setCoSlaPrio}
                      coSlaReason={coSlaReason}
                      setCoSlaReason={setCoSlaReason}
                      setCoErr={setCoErr}
                      loadCompanyOs={async () => {
                        await loadCompanyOs();
                      }}
                      filterPersona={focusAgentPersona}
                      onClearPersonaFilter={() => setFocusAgentPersona(null)}
                      dashboardFilter={taskDashboardFilter}
                      onClearDashboardFilter={() => setTaskDashboardFilter(null)}
                      scrollToTaskId={dashScrollTaskId}
                      onScrollToTaskDone={clearDashScrollTask}
                    />
                  </>
                ) : null}
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
