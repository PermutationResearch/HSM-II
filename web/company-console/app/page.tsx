"use client";

import Link from "next/link";
import {
  Building2,
  Download,
  Inbox,
  LayoutDashboard,
  LayoutList,
  Loader2,
  MonitorDot,
  Paperclip,
  Plus,
  Settings2,
  Store,
  Trash2,
  Upload,
  Users,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { OnboardingWizard, OnboardDraft } from "./components/OnboardingWizard";
import { CompanyAgentsPanel, type CoAgentRow } from "./components/CompanyAgentsPanel";
import { CompanyAgentSkillsPanel } from "./components/CompanyAgentSkillsPanel";
import { CompanyContextPanel } from "./components/CompanyContextPanel";
import { CompanySkillsPanel } from "./components/CompanySkillsPanel";
import { CompanyYcBenchPanel } from "./components/CompanyYcBenchPanel";
import { PolicyQueuePanel } from "./components/PolicyQueuePanel";
import type { QueueView } from "./lib/inboxPlainLanguage";
import { TaskListPanel, type TaskListDashboardFilter } from "./components/TaskListPanel";
import { GoalGovernancePanel } from "./components/GoalGovernancePanel";
import { OrchestrationPanels } from "./components/OrchestrationPanels";
import { AntiSycophancyPanel } from "./components/AntiSycophancyPanel";
import { CouncilSocraticPanel } from "./components/CouncilSocraticPanel";
import { SopComposerPanel } from "./components/SopComposerPanel";
import { PackMarketplacePanel } from "./components/PackMarketplacePanel";
import { CompanySharedMemoryPanel } from "./components/CompanySharedMemoryPanel";
import { SopReferenceExamples } from "./components/SopReferenceExamples";
import { sopReferenceExamples } from "./lib/sop-examples";
import type { SopExampleDocument } from "./lib/sop-examples-types";
import { loadCustomSops } from "./lib/sop-storage";
import { TrailGraphView, type GraphLink, type GraphNode, type TrailGraphPayload } from "./components/TrailGraphView";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "./components/ui/tabs";
import { useCompaniesShCatalog, type CompaniesShItem } from "../ui/src/hooks/useCompaniesShCatalog";
import { WorkspaceSidebar, type WorkspaceConsoleView } from "../ui/src/components/WorkspaceSidebar";
import { Dashboard, type DashboardDrillDown, type DashboardDrillQueueView } from "../ui/src/pages/Dashboard";
import { getConsoleApiBase } from "./lib/console-api-base";
import { createFromCatalogItem } from "./lib/create-from-catalog";
import { cn } from "./lib/utils";

type CoWorkspaceTab = "inbox" | "tasks" | "packs" | "sops" | "team" | "advanced";

const CO_WORKSPACE_TAB_ICONS = {
  inbox: Inbox,
  tasks: LayoutList,
  packs: Store,
  sops: Paperclip,
  team: Users,
  advanced: Settings2,
} as const;

function companyTabForDashboardDrill(d: DashboardDrillDown): CoWorkspaceTab {
  if (d.type === "spend") return "advanced";
  if (d.type === "inbox") return "inbox";
  if (d.type === "queue") {
    const v = d.view as DashboardDrillQueueView;
    if (v === "waiting_admin" || v === "pending_approvals" || v === "blocked") return "inbox";
  }
  return "tasks";
}

function downloadJson(filename: string, data: unknown) {
  const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = filename;
  a.click();
  URL.revokeObjectURL(a.href);
}


type TrailResp = { lines: Record<string, unknown>[]; path: string };
type Stats = {
  home: string;
  trail_lines: number;
  memory_markdown_files: number;
  agents_enabled: number;
  tasks_in_progress: number;
  company_os?: boolean;
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
  | "email"
  | "marketplace"
  | "sops";

type CompanyRow = {
  id: string;
  slug: string;
  display_name: string;
  hsmii_home?: string | null;
  issue_key_prefix?: string;
  context_markdown?: string | null;
  created_at: string;
};
type CoHealth = { postgres_configured: boolean; postgres_ok: boolean };
type TaskRow = {
  id: string;
  title: string;
  state: string;
  specification?: string | null;
  project_id?: string | null;
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
  /** Agent or workflow set — surfaces in human inbox without forcing waiting_admin state */
  requires_human?: boolean;
  parent_task_id?: string | null;
  spawned_by_rule_id?: string | null;
  /** Relative to company `hsmii_home` — injected into task LLM context */
  workspace_attachment_paths?: unknown;
  run?: {
    status: string;
    tool_calls: number;
    log_tail: string;
    finished_at?: string | null;
    updated_at?: string;
  } | null;
};
type GoalRowUi = {
  id: string;
  company_id?: string;
  parent_goal_id: string | null;
  title: string;
  description?: string | null;
  status: string;
};
type ProjectRowUi = {
  id: string;
  title: string;
  sort_order?: number;
  status?: string;
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
  const api = useMemo(() => getConsoleApiBase(), []);
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
  const [focusProjectId, setFocusProjectId] = useState<string | null>(null);
  const [taskDashboardFilter, setTaskDashboardFilter] = useState<TaskListDashboardFilter | null>(null);
  const [dashScrollTaskId, setDashScrollTaskId] = useState<string | null>(null);
  const [coSpendOpen, setCoSpendOpen] = useState(false);
  const [coTasks, setCoTasks] = useState<TaskRow[]>([]);
  const [coErr, setCoErr] = useState<string | null>(null);
  /** Shown after a successful Paperclip/directory import (agents + skills index). */
  const [coPackImportOk, setCoPackImportOk] = useState<string | null>(null);
  const [deletingWorkspaceId, setDeletingWorkspaceId] = useState<string | null>(null);
  const [coNewSlug, setCoNewSlug] = useState("");
  const [coNewName, setCoNewName] = useState("");
  const [coNewTaskTitle, setCoNewTaskTitle] = useState("");
  const [coNewTaskSpec, setCoNewTaskSpec] = useState("");
  const [coNewTaskProjectId, setCoNewTaskProjectId] = useState("");
  const [coNewTaskWorkspacePaths, setCoNewTaskWorkspacePaths] = useState("");
  const [coNewProjectTitle, setCoNewProjectTitle] = useState("");
  const [coGoals, setCoGoals] = useState<GoalRowUi[]>([]);
  const [coProjects, setCoProjects] = useState<ProjectRowUi[]>([]);
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
  const [coQueueView, setCoQueueView] = useState<QueueView>("human_inbox");
  const [coQueueTasks, setCoQueueTasks] = useState<TaskRow[]>([]);
  const [coFocusedSkillRequest, setCoFocusedSkillRequest] = useState<{ slug: string; nonce: number } | null>(null);
  /** Splits Company OS into daily work vs team setup vs power tools. */
  const [coWorkspaceTab, setCoWorkspaceTab] = useState<CoWorkspaceTab>("inbox");
  /** Workspace-scoped custom SOP templates (browser localStorage); merged into reference tabs. */
  const [customSops, setCustomSops] = useState<SopExampleDocument[]>([]);
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

  useEffect(() => {
    if (!coSel) {
      setCustomSops([]);
      return;
    }
    setCustomSops(loadCustomSops(coSel));
  }, [coSel]);

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
          setCoProjects([]);
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
          const [g, proj, t, ag, gov, sp, pr, q] = await Promise.all([
            fetch(`${api}/api/company/companies/${cid}/goals`).then((r) => {
              if (!r.ok) throw new Error(`goals ${r.status}`);
              return r.json() as Promise<{ goals: GoalRowUi[] }>;
            }),
            fetch(`${api}/api/company/companies/${cid}/projects`).then(async (r) => {
              // Older hsm_console builds or routing glitches may 404; keep the rest of Company OS usable.
              if (r.status === 404) return { projects: [] as ProjectRowUi[] };
              if (!r.ok) throw new Error(`projects ${r.status}`);
              return r.json() as Promise<{ projects: ProjectRowUi[] }>;
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
          setCoProjects(proj.projects ?? []);
          setCoTasks(t.tasks ?? []);
          setCoAgents(ag.agents ?? []);
          setCoGovernance(gov.events ?? []);
          setCoSpend(sp);
          setCoPolicyRules(pr.rules ?? []);
          setCoQueueTasks(q.tasks ?? []);
        } else {
          setCoTasks([]);
          setCoGoals([]);
          setCoProjects([]);
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

  useEffect(() => {
    setFocusProjectId(null);
    setCoNewTaskProjectId("");
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
      setCoWorkspaceTab(companyTabForDashboardDrill(d));

      if (!coSel) return;

      const q = async (v: QueueView) => {
        setCoQueueView(v);
        await loadQueueView(v);
      };

      switch (d.type) {
        case "inbox":
          await q("human_inbox");
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
      await createFromCatalogItem({
        apiBase: api,
        postgresConfigured: !!coHealth?.postgres_configured,
        item,
        setError: setCoErr,
        setPackImportOk: setCoPackImportOk,
        selectCompany: async (id) => {
          setCoSel(id);
          await loadCompanyOs(id);
        },
        afterPaperclipTeamOpen: () => {
          setView("company");
          requestAnimationFrame(() => setCoWorkspaceTab("team"));
        },
      });
    },
    [api, coHealth?.postgres_configured, loadCompanyOs, setView, setCoWorkspaceTab]
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
    setCoWorkspaceTab("inbox");
  }, [coSel]);

  const workspaceLabel = coCompanies.find((c) => c.id === coSel)?.display_name ?? "Workspace";
  const workspaceInitial = workspaceLabel.replace(/\s+/g, "").slice(0, 1) || "W";

  const sidebarAgents = useMemo(() => {
    const m = new Map<string, { id: string; name: string; liveCount: number; registryAgentId: string | null }>();
    for (const a of coAgents) {
      if (a.status === "terminated") continue;
      m.set(a.name, { id: a.name, name: a.name, liveCount: 0, registryAgentId: a.id });
    }
    for (const t of coTasks) {
      const id = (t.owner_persona ?? t.checked_out_by ?? "").trim();
      if (!id) continue;
      if (!m.has(id)) m.set(id, { id, name: id, liveCount: 0, registryAgentId: null });
      const row = m.get(id)!;
      if (t.checked_out_by || /progress|doing|active/i.test(t.state)) row.liveCount += 1;
    }
    return Array.from(m.values());
  }, [coTasks, coAgents]);

  const deleteRegistryAgentFromSidebar = useCallback(
    async (registryAgentId: string, personaId: string) => {
      if (!coSel) return;
      if (
        !window.confirm(
          `Remove workforce agent "${personaId}" from this company?\n\nDirect reports move to the top of the org (their manager link is cleared). Tasks that still reference this name as owner_persona are unchanged.`
        )
      ) {
        return;
      }
      setCoErr(null);
      try {
        const r = await fetch(`${api}/api/company/companies/${coSel}/agents/${registryAgentId}`, { method: "DELETE" });
        const j = (await r.json()) as { error?: string };
        if (!r.ok) throw new Error(j.error ?? r.statusText);
        setFocusAgentPersona((cur) => (cur === personaId ? null : cur));
        await loadCompanyOs();
      } catch (e) {
        setCoErr(e instanceof Error ? e.message : String(e));
      }
    },
    [api, coSel, loadCompanyOs]
  );

  const openTeamRolesForAgents = useCallback(() => {
    setView("company");
    setCoWorkspaceTab("team");
  }, []);

  const deleteCompanyFromSidebar = useCallback(
    async (c: { id: string; slug: string; display_name: string }) => {
      if (!coHealth?.postgres_configured) return;
      if (
        !window.confirm(
          `Delete workspace "${c.display_name}"?\n\nSlug: ${c.slug}\n\nAll tasks, goals, workforce agents, governance, and spend rows for this workspace are removed from the database. Pack files on disk (if any) are not deleted.`
        )
      ) {
        return;
      }
      setCoErr(null);
      setCoPackImportOk(null);
      try {
        const r = await fetch(
          `${api}/api/company/companies/${encodeURIComponent(c.id)}?confirm_slug=${encodeURIComponent(c.slug)}`,
          { method: "DELETE" }
        );
        const raw = await r.text();
        let msg: string | undefined;
        if (raw.trim()) {
          try {
            const j = JSON.parse(raw) as { error?: string; ok?: boolean };
            msg = j.error;
          } catch {
            msg = raw.slice(0, 280);
          }
        }
        if (!r.ok) {
          if (r.status === 405) {
            throw new Error(
              "Server rejected DELETE (405). Restart or rebuild hsm_console so it includes DELETE /api/company/companies/{id}."
            );
          }
          if (r.status === 404) {
            throw new Error(
              msg ||
                "Workspace not found (already removed?). If it keeps failing, run hsm_console and restart Next after changing HSM_CONSOLE_URL."
            );
          }
          throw new Error(msg || `${r.status} ${r.statusText || "Request failed"}`);
        }
        if (c.id === coSel) {
          setCoSel(null);
        }
        await loadCompanyOs();
      } catch (e) {
        setCoErr(e instanceof Error ? e.message : String(e));
      }
    },
    [api, coHealth?.postgres_configured, coSel, loadCompanyOs]
  );

  const dashboardLiveCount = useMemo(
    () => coTasks.filter((t) => t.checked_out_by || /progress|doing|active/i.test(t.state)).length,
    [coTasks]
  );

  /** Paperclip-style: only items that need a human (not total open tasks). */
  const inboxCount = useMemo(() => {
    return coTasks.filter(
      (t) =>
        t.requires_human === true ||
        t.state === "waiting_admin" ||
        t.state === "blocked"
    ).length;
  }, [coTasks]);

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

  const coProjectsSorted = useMemo(
    () =>
      [...coProjects].sort(
        (a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0) || a.title.localeCompare(b.title)
      ),
    [coProjects]
  );

  const sidebarProjects = useMemo(
    () => coProjectsSorted.map((p) => ({ id: p.id, title: p.title })),
    [coProjectsSorted]
  );

  const projectTitleById = useMemo(() => {
    const m: Record<string, string> = {};
    for (const p of coProjects) m[p.id] = p.title;
    return m;
  }, [coProjects]);

  const sidebarGoals = useMemo(
    () => coGoalsSorted.map((g) => ({ id: g.id, title: g.title })),
    [coGoalsSorted]
  );

  const focusProjectMeta = useMemo(() => {
    const id = focusProjectId?.trim();
    if (!id) return null;
    const p = coProjects.find((x) => x.id === id);
    return { id, title: p?.title ?? id };
  }, [focusProjectId, coProjects]);

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
        companies={coCompanies.map((c) => ({
          id: c.id,
          display_name: c.display_name,
          slug: c.slug,
          hsmii_home: c.hsmii_home,
        }))}
        selectedCompanyId={coSel}
        onSelectCompany={(id) => {
          setCoPackImportOk(null);
          setCoSel(id);
          void loadCompanyOs(id);
        }}
        view={view as WorkspaceConsoleView}
        onNavigate={(id) => setView(id as NavId)}
        dashboardLiveCount={dashboardLiveCount}
        inboxCount={inboxCount}
        projects={sidebarProjects}
        selectedProjectId={focusProjectId}
        onSelectProject={(id) => {
          setFocusProjectId(id);
          setFocusAgentPersona(null);
        }}
        goals={sidebarGoals}
        agents={sidebarAgents}
        selectedAgentPersona={focusAgentPersona}
        companyWorkTab={view === "company" ? coWorkspaceTab : null}
        onNavigateInbox={() => {
          setFocusProjectId(null);
          setView("company");
          setCoWorkspaceTab("inbox");
          setCoQueueView("human_inbox");
          void loadQueueView("human_inbox");
        }}
        onNavigateTasks={() => {
          setView("company");
          setCoWorkspaceTab("tasks");
        }}
        onSelectAgent={(persona) => {
          setFocusProjectId(null);
          setFocusAgentPersona(persona);
          setView("company");
          setCoWorkspaceTab("tasks");
        }}
        onDeleteRegistryAgent={coSel ? deleteRegistryAgentFromSidebar : undefined}
        onAddRegistryAgent={coSel ? openTeamRolesForAgents : undefined}
        onNewIssue={() => {
          setView("company");
          setCoWorkspaceTab("tasks");
        }}
        onOpenOnboarding={() => setView("onboard")}
        onDeleteCompany={coHealth?.postgres_configured ? deleteCompanyFromSidebar : undefined}
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
            ? "nd-dashboard-shell min-h-screen min-w-0 flex-1 overflow-auto bg-[#010409] px-6 py-5"
            : "nd-dashboard-shell min-h-screen min-w-0 flex-1 overflow-auto px-6 py-5"
        }
      >
        <div className="mb-4 flex flex-wrap items-center justify-between gap-2 rounded-lg border border-[#30363D] bg-[#0d1117] px-3 py-2 text-sm text-[#8B949E]">
          <span>
            Workspace UI matches this console: dashboard, agents, issues,{" "}
            <Link href="/workspace/marketplace" className="font-mono text-[#58a6ff] underline-offset-4 hover:underline">
              marketplace
            </Link>
            ,{" "}
            <Link href="/workspace/playbooks" className="font-mono text-[#58a6ff] underline-offset-4 hover:underline">
              playbooks
            </Link>
            , intelligence —{" "}
            <Link href="/workspace/dashboard" className="font-mono text-[#58a6ff] underline-offset-4 hover:underline">
              /workspace
            </Link>
          </span>
        </div>
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
            {coPackImportOk && (
              <div className="mb-4 rounded border border-emerald-900/50 bg-emerald-950/25 px-3 py-2 text-sm text-emerald-100/95">
                {coPackImportOk}
              </div>
            )}
            <div className="mb-4 flex flex-wrap items-center justify-end gap-2 border-b border-[#222222] pb-4">
              <span className="mr-auto flex items-center gap-2 font-mono text-[10px] uppercase tracking-[0.08em] text-[#666666]">
                <Paperclip className="h-3.5 w-3.5 text-[#8B949E]" aria-hidden />
                Dashboard view
              </span>
              <div className="inline-flex gap-px rounded-md border border-[#30363D] bg-[#0d1117] p-px">
                <button
                  type="button"
                  onClick={() => persistCommandDashboardLayout("nothing")}
                  className={cn(
                    "inline-flex items-center gap-1.5 rounded-sm px-3 py-1.5 font-mono text-[11px] font-medium uppercase tracking-wide transition-colors",
                    commandDashboardLayout === "nothing"
                      ? "bg-[#21262d] text-white ring-1 ring-white/10"
                      : "text-[#999999] hover:bg-white/5 hover:text-white"
                  )}
                >
                  <LayoutDashboard className="h-3.5 w-3.5 opacity-80" aria-hidden />
                  Overview
                </button>
                <button
                  type="button"
                  onClick={() => persistCommandDashboardLayout("admin")}
                  className={cn(
                    "inline-flex items-center gap-1.5 rounded-sm px-3 py-1.5 font-mono text-[11px] font-medium uppercase tracking-wide transition-colors",
                    commandDashboardLayout === "admin"
                      ? "bg-[#21262d] text-white ring-1 ring-white/10"
                      : "text-[#999999] hover:bg-white/5 hover:text-white"
                  )}
                >
                  <MonitorDot className="h-3.5 w-3.5 opacity-80" aria-hidden />
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
        {view === "quality" && (
          <div className="space-y-4">
            <h1 className="text-xl font-medium text-white">Quality &amp; debate</h1>
            <Tabs defaultValue="council" className="w-full">
              <TabsList variant="line" className="mb-4 w-full max-w-xl justify-start">
                <TabsTrigger value="council">Socratic council</TabsTrigger>
                <TabsTrigger value="anti">Anti-sycophancy only</TabsTrigger>
              </TabsList>
              <TabsContent value="council" className="mt-0">
                <CouncilSocraticPanel api={api} setErr={setErr} />
              </TabsContent>
              <TabsContent value="anti" className="mt-0">
                <AntiSycophancyPanel api={api} setErr={setErr} />
              </TabsContent>
            </Tabs>
          </div>
        )}
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
              setCoPackImportOk(null);
              setCoSel(cid);
              setView("company");
              setCoWorkspaceTab("inbox");
              await loadCompanyOs(cid);
            }}
          />
        )}
        {view === "company" && (
          <>
            <header className="mb-6 border-b border-[#30363D] pb-5">
              <p className="flex items-center gap-2 font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-[#8B949E]">
                <Paperclip className="h-4 w-4 text-[#58a6ff]/90" aria-hidden />
                Company OS
              </p>
              <h1 className="mt-1 text-lg font-medium tracking-tight text-white">
                {coWorkspaceTab === "inbox"
                  ? "Inbox"
                  : coWorkspaceTab === "tasks"
                    ? "Tasks"
                    : coWorkspaceTab === "packs"
                      ? "Pack marketplace"
                      : coWorkspaceTab === "sops"
                        ? "SOPs & playbooks"
                        : coWorkspaceTab === "team"
                          ? "Team & setup"
                          : "Advanced"}
              </h1>
              <p className="mt-2 max-w-2xl text-sm leading-relaxed text-[#8B949E]">
                {coWorkspaceTab === "inbox"
                  ? "Decision feed: agents work on their own and surface items here when they need you—approve, reject, or reply; cleared like email. Your job is to unblock them, not to manage every task."
                  : coWorkspaceTab === "tasks"
                    ? "Full task graph: create work, check out, SLA, filters—the operational backlog. Urgent human decisions live in Inbox, not here."
                    : coWorkspaceTab === "packs"
                      ? "Browse agent-company templates from the open directory, see what each pack is for, add a workspace, or send a new pack idea for listing."
                      : coWorkspaceTab === "sops"
                        ? "Author standard operating procedures: phases, escalation, and governance log templates—then implement them as tasks in this workspace."
                        : coWorkspaceTab === "team"
                          ? "Configure agents and knowledge here. Day-to-day approvals and escalations are in Inbox — this tab is backstage, not the decision feed."
                          : "Backup, spend, goals, policies, orchestration, and power tools."}
              </p>
            </header>
            <div className="mb-4 max-w-3xl">
              <div className="inline-flex max-w-full flex-wrap gap-px rounded-md border border-[#30363D] bg-[#0d1117] p-px">
                {(
                  [
                    ["inbox", "Inbox"],
                    ["tasks", "Tasks"],
                    ["packs", "Pack marketplace"],
                    ["sops", "SOPs & playbooks"],
                    ["team", "Team & setup"],
                    ["advanced", "Advanced"],
                  ] as const
                ).map(([id, label]) => {
                  const Icon = CO_WORKSPACE_TAB_ICONS[id];
                  return (
                    <button
                      key={id}
                      type="button"
                      onClick={() => {
                        setCoWorkspaceTab(id);
                        if (id === "inbox") {
                          setCoQueueView("human_inbox");
                          void loadQueueView("human_inbox");
                        }
                      }}
                      className={cn(
                        "inline-flex items-center gap-1.5 rounded-sm px-3 py-1.5 text-sm font-medium transition-colors",
                        coWorkspaceTab === id
                          ? "bg-[#21262d] text-white ring-1 ring-[#30363d]"
                          : "text-gray-400 hover:bg-white/[0.06] hover:text-white"
                      )}
                    >
                      <Icon className="h-3.5 w-3.5 shrink-0 opacity-85" aria-hidden />
                      {label}
                    </button>
                  );
                })}
              </div>
              <p className="mt-2 text-xs leading-relaxed text-gray-600">
                <strong className="font-medium text-gray-500">Inbox</strong> — human decisions only (approvals, blocks,
                agent escalations). <strong className="font-medium text-gray-500">Tasks</strong> — full list and
                operations. <strong className="font-medium text-gray-500">Pack marketplace</strong> — directory + propose
                new packs. <strong className="font-medium text-gray-500">SOPs &amp; playbooks</strong> — design procedures
                and materialize them.                 <strong className="font-medium text-gray-500">Team &amp; setup</strong> — roster, skills, shared
                context. <strong className="font-medium text-gray-500">Advanced</strong> — backup, spend, goals,
                orchestration.
              </p>
            </div>
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
            {coPackImportOk && (
              <div className="mb-4 rounded border border-emerald-900/50 bg-emerald-950/25 px-3 py-2 text-sm text-emerald-100/95">
                {coPackImportOk}
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
                  className="inline-flex items-center gap-2 rounded-md border border-[#30363d] bg-[#21262d] px-4 py-2 text-sm font-medium text-white hover:bg-[#30363d]"
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
                  <Building2 className="h-4 w-4 opacity-90" aria-hidden />
                  Create workspace
                </button>
              </div>
            )}
            {coCompanies.length > 0 && (
              <div className="mb-6 overflow-hidden rounded-xl border border-[#30363D] bg-[#0d1117]">
                <div className="border-b border-[#30363D] px-4 py-3">
                  <p className="mb-2 font-mono text-[10px] font-semibold uppercase tracking-[0.12em] text-[#6e7681]">
                    Workspace
                  </p>
                  <div className="flex flex-wrap items-center gap-3">
                    <Building2 className="h-4 w-4 shrink-0 text-[#8B949E]" aria-hidden />
                    <select
                      value={coSel ?? ""}
                      onChange={(e) => {
                        setCoPackImportOk(null);
                        setCoSel(e.target.value);
                        void loadCompanyOs(e.target.value);
                      }}
                      className="min-w-[200px] flex-1 rounded-lg border border-[#30363D] bg-[#010409] px-3 py-2.5 text-sm text-[#E8E8E8] outline-none focus:border-[#58a6ff] sm:max-w-xs sm:flex-none"
                      aria-label="Active workspace"
                    >
                      <option value="" disabled>
                        Select workspace…
                      </option>
                      {coCompanies.map((c) => (
                        <option key={c.id} value={c.id}>
                          {c.display_name}
                        </option>
                      ))}
                    </select>
                    {coSel && coHealth?.postgres_configured && (() => {
                      const sel = coCompanies.find((c) => c.id === coSel);
                      if (!sel) return null;
                      return (
                        <button
                          type="button"
                          title={`Permanently delete ${sel.display_name} from the database`}
                          disabled={!!deletingWorkspaceId}
                          className="inline-flex items-center gap-2 rounded-lg border border-[#30363D] px-3 py-2 text-sm text-[#8B949E] transition-colors hover:border-[#f85149]/50 hover:bg-[#f85149]/10 hover:text-[#ffa198] disabled:cursor-not-allowed disabled:opacity-40"
                          onClick={() => {
                            void (async () => {
                              setDeletingWorkspaceId(sel.id);
                              try {
                                await deleteCompanyFromSidebar({
                                  id: sel.id,
                                  slug: sel.slug,
                                  display_name: sel.display_name,
                                });
                              } finally {
                                setDeletingWorkspaceId(null);
                              }
                            })();
                          }}
                        >
                          {deletingWorkspaceId === sel.id ? (
                            <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
                          ) : (
                            <Trash2 className="h-4 w-4 stroke-[1.5]" aria-hidden />
                          )}
                          Delete workspace
                        </button>
                      );
                    })()}
                  </div>
                </div>
                <div className="px-4 py-3">
                  <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                    <div className="min-w-0 max-w-xl">
                      <label
                        className="font-mono text-[10px] font-semibold uppercase tracking-[0.12em] text-[#6e7681]"
                        htmlFor="co-checkout-agent"
                      >
                        Your actor ID
                      </label>
                      <p className="mt-1 text-xs leading-relaxed text-[#8B949E]">
                        Logged on <strong className="font-medium text-[#c9d1d9]">Inbox</strong> actions (approve / reject
                        / clear flag), task checkouts, and governance events. Use a stable id (e.g. your name or{" "}
                        <code className="rounded bg-white/5 px-1 font-mono text-[11px]">agent-1</code> for testing).
                      </p>
                    </div>
                    <input
                      id="co-checkout-agent"
                      className="h-10 w-full shrink-0 rounded-lg border border-[#30363D] bg-[#010409] px-3 font-mono text-sm text-[#E8E8E8] outline-none focus:border-[#58a6ff] sm:w-52"
                      placeholder="e.g. jamie or agent-1"
                      value={coCheckoutAgent}
                      onChange={(e) => setCoCheckoutAgent(e.target.value)}
                      autoComplete="username"
                    />
                  </div>
                </div>
                <details className="border-t border-[#30363D] border-dashed bg-[#010409]/40">
                  <summary className="cursor-pointer px-4 py-2.5 text-xs text-[#8B949E] hover:text-[#c9d1d9]">
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
                      className="inline-flex items-center gap-1.5 rounded-md border border-[#30363d] bg-[#161b22] px-3 py-1.5 text-sm text-gray-200 hover:bg-[#21262d]"
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
                      <Building2 className="h-3.5 w-3.5 opacity-80" aria-hidden />
                      Add
                    </button>
                  </div>
                </details>
              </div>
            )}
            {coWorkspaceTab === "packs" ? (
              <PackMarketplacePanel
                items={companiesSh.items}
                loading={companiesSh.loading}
                error={companiesSh.error}
                postgresConfigured={!!coHealth?.postgres_configured}
                companies={coCompanies.map((c) => ({
                  id: c.id,
                  slug: c.slug,
                  hsmii_home: c.hsmii_home,
                }))}
                onCreateFromCatalog={createFromCatalog}
                setCoErr={setCoErr}
              />
            ) : null}
            {coSel ? (
              <>
                {coWorkspaceTab === "advanced" ? (
                  <>
                    <details className="mb-4 rounded-lg border border-dashed border-line/80 bg-black/20">
                      <summary className="cursor-pointer px-3 py-2 text-xs text-gray-500 hover:text-gray-400">
                        Backup, import &amp; export
                      </summary>
                      <p className="border-t border-line/60 px-3 py-2 text-xs leading-relaxed text-gray-600">
                        Each company is a namespaced API under{" "}
                        <code className="font-mono text-[11px] text-gray-500">
                          /api/company/companies/&lt;id&gt;/…
                        </code>
                        . Use <strong className="font-medium text-gray-500">API catalog</strong> to list routes for
                        integrators.
                      </p>
                      <div className="flex flex-wrap items-center gap-3 border-t border-line/60 px-3 py-3">
                        <button
                          type="button"
                          className="inline-flex items-center gap-1.5 rounded-md border border-[#30363d] bg-[#161b22] px-3 py-1.5 text-sm text-gray-200 hover:bg-[#21262d]"
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
                          <Download className="h-3.5 w-3.5 opacity-90" aria-hidden />
                          Export JSON
                        </button>
                        <button
                          type="button"
                          className="inline-flex items-center gap-1.5 rounded-md border border-[#30363d] bg-[#161b22] px-3 py-1.5 text-sm text-gray-200 hover:bg-[#21262d]"
                          title="Machine-readable list of Company OS HTTP routes for this workspace"
                          onClick={async () => {
                            setCoErr(null);
                            try {
                              const r = await fetch(`${api}/api/company/companies/${coSel}/api-catalog`);
                              const j = await r.json();
                              if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                              downloadJson(`company-api-catalog-${coSel.slice(0, 8)}.json`, j);
                            } catch (e) {
                              setCoErr(e instanceof Error ? e.message : String(e));
                            }
                          }}
                        >
                          <Download className="h-3.5 w-3.5 opacity-90" aria-hidden />
                          Download API catalog
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
                                setCoPackImportOk(null);
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
                          className="inline-flex items-center gap-1.5 rounded-md border border-[#30363d] bg-[#161b22] px-3 py-1.5 text-sm text-gray-200 hover:bg-[#21262d]"
                          onClick={() => coImportRef.current?.click()}
                        >
                          <Upload className="h-3.5 w-3.5 opacity-90" aria-hidden />
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

                {coWorkspaceTab === "sops" ? (
                  coSel ? (
                    <div className="space-y-6">
                      <SopComposerPanel
                        apiBase={api}
                        companyId={coSel}
                        referenceExamples={sopReferenceExamples}
                        onCustomSopsChanged={setCustomSops}
                        onApplied={async () => {
                          await loadCompanyOs();
                        }}
                        setCoErr={setCoErr}
                      />
                      <div>
                        <h2 className="mb-2 text-sm font-semibold text-white">Reference &amp; saved library</h2>
                        <p className="mb-4 max-w-3xl text-xs leading-relaxed text-gray-500">
                          Built-in examples plus templates you saved for this workspace (tabs marked{" "}
                          <span className="rounded border border-line px-1 text-[10px] text-gray-400">yours</span>). Use{" "}
                          <strong className="font-medium text-gray-400">Implement in workspace</strong> to create
                          playbook tasks and governance seeds.
                        </p>
                        <SopReferenceExamples
                          apiBase={api}
                          companyId={coSel}
                          onApplied={async () => {
                            await loadCompanyOs();
                          }}
                          setCoErr={setCoErr}
                          additionalExamples={customSops}
                        />
                      </div>
                    </div>
                  ) : (
                    <div className="rounded-lg border border-amber-900/40 bg-amber-950/20 px-4 py-3 text-sm text-amber-100/90">
                      Select or create a <strong className="font-medium text-white">workspace</strong> above to author
                      SOPs, save templates for this company, and implement playbooks.
                    </div>
                  )
                ) : null}

                {coWorkspaceTab === "team" ? (
                  <>
                    <div className="mb-6 rounded-xl border border-[#30363D] bg-gradient-to-br from-[#388bfd]/15 via-[#0d1117] to-[#0d1117] px-4 py-4">
                      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                        <div className="min-w-0">
                          <h2 className="text-sm font-semibold text-white">Configure agents, not the inbox</h2>
                          <p className="mt-1 max-w-2xl text-xs leading-relaxed text-[#8B949E]">
                            This screen is <strong className="font-medium text-[#c9d1d9]">backstage</strong>: shared
                            knowledge, imported skills, and roster. Agents run work elsewhere; when they need a person, items
                            appear under <strong className="font-medium text-[#c9d1d9]">Inbox</strong> in the sidebar.
                          </p>
                        </div>
                        <button
                          type="button"
                          className="inline-flex shrink-0 items-center justify-center gap-2 rounded-lg border border-[#58a6ff]/45 bg-[#388bfd]/15 px-4 py-2.5 text-sm font-medium text-[#79b8ff] transition-colors hover:bg-[#388bfd]/25"
                          onClick={() => {
                            setCoWorkspaceTab("inbox");
                            setCoQueueView("human_inbox");
                            void loadQueueView("human_inbox");
                          }}
                        >
                          <Inbox className="h-4 w-4 shrink-0" aria-hidden />
                          Open Inbox
                        </button>
                      </div>
                      <ol className="mt-4 grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
                        {(
                          [
                            ["1", "Shared context", "Stable facts for every agent via llm-context."],
                            ["2", "Skill templates", "What the pack indexed from disk."],
                            ["3", "Agent ↔ skills", "Who references which template."],
                            ["4", "Roster", "Org chart, adapters, budgets."],
                          ] as const
                        ).map(([n, t, d]) => (
                          <li
                            key={n}
                            className="rounded-lg border border-[#30363D]/90 bg-[#010409]/50 px-3 py-2.5 text-xs leading-snug text-[#8B949E]"
                          >
                            <span className="mr-1.5 inline-flex h-5 w-5 items-center justify-center rounded-full bg-[#21262d] font-mono text-[10px] font-bold text-[#58a6ff]">
                              {n}
                            </span>
                            <span className="font-medium text-[#c9d1d9]">{t}</span>
                            <span className="mt-1 block text-[11px] text-[#6e7681]">{d}</span>
                          </li>
                        ))}
                      </ol>
                    </div>
                    <CompanyContextPanel
                      api={api}
                      companyId={coSel}
                      contextMarkdown={coCompanies.find((c) => c.id === coSel)?.context_markdown}
                      setCoErr={setCoErr}
                      onSaved={async () => {
                        await loadCompanyOs();
                      }}
                    />
                    <CompanyYcBenchPanel
                      api={api}
                      companyId={coSel}
                      setCoErr={setCoErr}
                    />
                    <CompanySkillsPanel
                      api={api}
                      companyId={coSel}
                      setCoErr={setCoErr}
                      focusSkillSlug={coFocusedSkillRequest?.slug ?? null}
                      focusSkillNonce={coFocusedSkillRequest?.nonce}
                    />
                    <CompanyAgentSkillsPanel
                      api={api}
                      companyId={coSel}
                      agents={coAgents}
                      setCoErr={setCoErr}
                      onOpenSkill={(slug) =>
                        setCoFocusedSkillRequest((cur) => ({
                          slug,
                          nonce: (cur?.nonce ?? 0) + 1,
                        }))
                      }
                    />
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
                  </>
                ) : null}

                {coWorkspaceTab === "inbox" ? (
                  <>
                    <div className="mb-5 max-w-3xl rounded-lg border border-[#30363D] bg-[#0d1117] px-4 py-3 text-sm leading-relaxed text-[#8B949E]">
                      <p className="text-[#c9d1d9]">
                        Agents run autonomously; they only interrupt you when something needs a person. Typical lines look
                        like:
                      </p>
                      <ul className="mt-2 list-disc space-y-1 pl-5 text-[#8B949E]">
                        <li>
                          <span className="text-[#c9d1d9]">“Agent X needs approval to spend $500”</span>
                        </li>
                        <li>
                          <span className="text-[#c9d1d9]">“Ambiguous customer request — pick A or B”</span>
                        </li>
                        <li>
                          <span className="text-[#c9d1d9]">“Contract ready for your signature”</span>
                        </li>
                      </ul>
                      <p className="mt-2 text-xs text-[#6e7681]">
                        Approve, reject, or respond — the item leaves your inbox. Everything else is under{" "}
                        <strong className="font-medium text-[#8B949E]">Tasks</strong>.
                      </p>
                    </div>

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
                  </>
                ) : null}

                {coWorkspaceTab === "tasks" ? (
                  <>
                    <div className="mb-4 flex flex-wrap gap-4">
                      <div className="min-w-[240px] flex-1 rounded border border-line bg-panel p-4">
                        <div className="mb-1 text-sm font-semibold text-white">New project</div>
                        <p className="mb-3 text-xs text-gray-500">
                          Group related issues (Paperclip-style). Projects are separate from the strategic goal tree.
                        </p>
                        <input
                          className="mb-3 w-full rounded-lg border border-line bg-ink px-3 py-2 text-sm"
                          placeholder="Project name"
                          value={coNewProjectTitle}
                          onChange={(e) => setCoNewProjectTitle(e.target.value)}
                        />
                        <button
                          type="button"
                          className="inline-flex items-center gap-2 rounded-md border border-[#30363d] bg-[#21262d] px-4 py-2 text-sm font-medium text-white hover:bg-[#30363d]"
                          onClick={async () => {
                            setCoErr(null);
                            const title = coNewProjectTitle.trim();
                            if (!title) {
                              setCoErr("Project title required");
                              return;
                            }
                            try {
                              const r = await fetch(`${api}/api/company/companies/${coSel}/projects`, {
                                method: "POST",
                                headers: { "Content-Type": "application/json" },
                                body: JSON.stringify({ title }),
                              });
                              const j = await r.json();
                              if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                              setCoNewProjectTitle("");
                              await loadCompanyOs();
                            } catch (e) {
                              setCoErr(e instanceof Error ? e.message : String(e));
                            }
                          }}
                        >
                          <Plus className="h-4 w-4 opacity-90" aria-hidden />
                          Create project
                        </button>
                      </div>
                      <div className="min-w-[240px] flex-1 rounded border border-line bg-panel p-4">
                        <div className="mb-1 text-sm font-semibold text-white">New task</div>
                        <p className="mb-3 text-xs text-gray-500">
                          A single thing someone should do—or that you want help with.
                        </p>
                        <label className="mb-2 block font-mono text-[10px] uppercase tracking-wide text-gray-500">
                          Project (optional)
                          <select
                            className="mt-1 w-full rounded-lg border border-line bg-ink px-3 py-2 text-sm text-white"
                            value={coNewTaskProjectId}
                            onChange={(e) => setCoNewTaskProjectId(e.target.value)}
                          >
                            <option value="">— None —</option>
                            {coProjectsSorted.map((p) => (
                              <option key={p.id} value={p.id}>
                                {p.title}
                              </option>
                            ))}
                          </select>
                        </label>
                        <input
                          className="mb-2 w-full rounded-lg border border-line bg-ink px-3 py-2 text-sm"
                          placeholder="What needs to happen?"
                          value={coNewTaskTitle}
                          onChange={(e) => setCoNewTaskTitle(e.target.value)}
                        />
                        <textarea
                          className="mb-2 w-full rounded-lg border border-line bg-ink px-3 py-2 text-sm"
                          placeholder="Details or acceptance criteria (optional)"
                          rows={3}
                          value={coNewTaskSpec}
                          onChange={(e) => setCoNewTaskSpec(e.target.value)}
                        />
                        <label className="mb-3 block font-mono text-[10px] uppercase tracking-wide text-gray-500">
                          Workspace paths (optional, one per line — relative to company hsmii_home)
                          <textarea
                            className="mt-1 w-full rounded-lg border border-line bg-ink px-3 py-2 font-mono text-xs text-gray-300"
                            placeholder={"e.g. workspace/content/handbook.md"}
                            rows={2}
                            value={coNewTaskWorkspacePaths}
                            onChange={(e) => setCoNewTaskWorkspacePaths(e.target.value)}
                          />
                        </label>
                        <button
                          type="button"
                          className="inline-flex items-center gap-2 rounded-md border border-[#30363d] bg-[#21262d] px-4 py-2 text-sm font-medium text-white hover:bg-[#30363d]"
                          onClick={async () => {
                            setCoErr(null);
                            try {
                              const body: Record<string, unknown> = {
                                title: coNewTaskTitle.trim(),
                                specification: coNewTaskSpec.trim() || undefined,
                              };
                              const pid = coNewTaskProjectId.trim();
                              if (pid) body.project_id = pid;
                              const pathLines = coNewTaskWorkspacePaths
                                .split(/\r?\n/)
                                .map((s) => s.trim())
                                .filter(Boolean);
                              if (pathLines.length) body.workspace_attachment_paths = pathLines;
                              const r = await fetch(`${api}/api/company/companies/${coSel}/tasks`, {
                                method: "POST",
                                headers: { "Content-Type": "application/json" },
                                body: JSON.stringify(body),
                              });
                              const j = await r.json();
                              if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                              setCoNewTaskTitle("");
                              setCoNewTaskSpec("");
                              setCoNewTaskWorkspacePaths("");
                              await loadCompanyOs();
                            } catch (e) {
                              setCoErr(e instanceof Error ? e.message : String(e));
                            }
                          }}
                        >
                          <Plus className="h-4 w-4 opacity-90" aria-hidden />
                          Add to list
                        </button>
                      </div>
                    </div>

                    {focusProjectMeta ? (
                      <div className="mb-4 rounded-2xl border border-[#30363D] bg-[#0d1117] px-4 py-4">
                        <div className="flex flex-wrap items-start justify-between gap-3">
                          <div className="min-w-0">
                            <p className="font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-[#8B949E]">
                              Project view
                            </p>
                            <h2 className="mt-1 text-base font-semibold text-[#58a6ff]">{focusProjectMeta.title}</h2>
                            <p className="mt-2 max-w-2xl text-sm leading-relaxed text-[#8B949E]">
                              Showing tasks linked to this project. Pick another project in the sidebar or clear to see all
                              tasks.
                            </p>
                          </div>
                          <button
                            type="button"
                            className="shrink-0 rounded-md border border-[#30363D] px-3 py-2 font-mono text-[11px] font-medium uppercase tracking-wide text-[#c9d1d9] hover:border-[#484f58] hover:bg-[#161b22]"
                            onClick={() => setFocusProjectId(null)}
                          >
                            Show all tasks
                          </button>
                        </div>
                      </div>
                    ) : null}

                    {focusAgentMeta ? (
                      <div className="mb-4 rounded-2xl border border-[#30363D] bg-[#0d1117] px-4 py-4">
                        <div className="flex flex-wrap items-start justify-between gap-3">
                          <div className="min-w-0">
                            <p className="font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-[#8B949E]">
                              Agent view
                            </p>
                            <h2 className="mt-1 break-all font-mono text-base font-semibold text-[#58a6ff]">
                              {focusAgentMeta.id}
                            </h2>
                            <p className="mt-2 max-w-2xl text-sm leading-relaxed text-[#8B949E]">
                              Showing tasks owned by or checked out by this agent.
                            </p>
                            {focusAgentMeta.inRegistry ? (
                              <p className="mt-2 text-xs text-[#8B949E]">
                                {focusAgentMeta.role ? (
                                  <>Role: <span className="text-[#c9d1d9]">{focusAgentMeta.role}</span></>
                                ) : null}
                                {focusAgentMeta.title ? (
                                  <>
                                    {focusAgentMeta.role ? " · " : ""}
                                    <span className="text-[#c9d1d9]">{focusAgentMeta.title}</span>
                                  </>
                                ) : null}
                              </p>
                            ) : (
                              <p className="mt-2 text-xs text-[#484f58]">
                                Not yet in <strong className="text-[#6e7681]">Team &amp; setup</strong>. Add a row
                                there to give this agent a proper profile.
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
                            Activity log
                          </p>
                          <p className="mt-1 max-w-2xl text-xs leading-relaxed text-[#484f58]">
                            Actions performed by this agent (e.g. checking out tasks under this identity).
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
                            <p className="mt-3 text-xs text-[#484f58]">No activity yet.</p>
                          )}
                        </div>
                      </div>
                    ) : null}

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
                      filterProjectId={focusProjectId}
                      onClearProjectFilter={() => setFocusProjectId(null)}
                      projectTitles={projectTitleById}
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
            ) : coWorkspaceTab !== "packs" ? (
              <div className="rounded-lg border border-amber-900/40 bg-amber-950/20 px-4 py-3 text-sm text-amber-100/90">
                Select or create a <strong className="font-medium text-white">workspace</strong> above for tasks, SOPs,
                and team tools—or open <strong className="font-medium text-white">Pack marketplace</strong> to browse
                templates without selecting one yet.
              </div>
            ) : null}
          </>
        )}
        {view === "marketplace" && (
          <>
            <header className="mb-6 border-b border-[#30363D] pb-5">
              <h1 className="text-lg font-medium tracking-tight text-white">Marketplace</h1>
              <p className="mt-2 max-w-2xl text-sm leading-relaxed text-[#8B949E]">
                Browse agent-company templates from the open directory. Add a workspace to get started, or propose a new
                pack for listing.
              </p>
            </header>
            <PackMarketplacePanel
              items={companiesSh.items}
              loading={companiesSh.loading}
              error={companiesSh.error}
              postgresConfigured={!!coHealth?.postgres_configured}
              companies={coCompanies.map((c) => ({
                id: c.id,
                slug: c.slug,
                hsmii_home: c.hsmii_home,
              }))}
              onCreateFromCatalog={createFromCatalog}
              setCoErr={setCoErr}
            />
          </>
        )}
        {view === "sops" && (
          <>
            <header className="mb-6 border-b border-[#30363D] pb-5">
              <h1 className="text-lg font-medium tracking-tight text-white">Playbooks</h1>
              <p className="mt-2 max-w-2xl text-sm leading-relaxed text-[#8B949E]">
                Design standard operating procedures, escalation steps, and governance templates—then implement them
                directly as tasks in your workspace.
              </p>
            </header>
            {coSel ? (
              <div className="space-y-6">
                <SopComposerPanel
                  apiBase={api}
                  companyId={coSel}
                  referenceExamples={sopReferenceExamples}
                  onCustomSopsChanged={setCustomSops}
                  onApplied={async () => {
                    await loadCompanyOs();
                  }}
                  setCoErr={setCoErr}
                />
                <div>
                  <h2 className="mb-2 text-sm font-semibold text-white">Reference &amp; saved library</h2>
                  <p className="mb-4 max-w-3xl text-xs leading-relaxed text-gray-500">
                    Built-in examples plus templates you saved for this workspace (tabs marked{" "}
                    <span className="rounded border border-line px-1 text-[10px] text-gray-400">yours</span>). Use{" "}
                    <strong className="font-medium text-gray-400">Implement in workspace</strong> to create playbook
                    tasks and governance seeds.
                  </p>
                  <SopReferenceExamples
                    apiBase={api}
                    companyId={coSel}
                    onApplied={async () => {
                      await loadCompanyOs();
                    }}
                    setCoErr={setCoErr}
                    additionalExamples={customSops}
                  />
                </div>
              </div>
            ) : (
              <div className="rounded-lg border border-amber-900/40 bg-amber-950/20 px-4 py-3 text-sm text-amber-100/90">
                Select a <strong className="font-medium text-white">workspace</strong> from the sidebar first to author
                SOPs and implement playbooks.
              </div>
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
            <h1 className="mb-2 text-lg font-medium text-white">Memory</h1>
            <p className="mb-6 max-w-3xl text-sm text-gray-500">
              <strong className="font-medium text-gray-300">Company shared memory</strong> lives in Postgres and is merged
              into task LLM context. <strong className="font-medium text-gray-300">Local memory/</strong> files below are
              on-disk snippets from the console host (Hermes-style).
            </p>

            <section className="mb-10">
              <h2 className="mb-3 text-sm font-semibold uppercase tracking-wide text-gray-400">Workspace shared pool</h2>
              <CompanySharedMemoryPanel
                apiBase={api}
                companyId={coSel}
                postgresOk={!!(coHealth?.postgres_configured && coHealth?.postgres_ok)}
                onError={(msg) => setCoErr(msg)}
              />
            </section>

            <section>
              <h2 className="mb-3 text-sm font-semibold uppercase tracking-wide text-gray-400">Local memory/ markdown</h2>
              <p className="mb-4 text-sm text-gray-400">{memoryFiles?.count ?? 0} markdown files under memory/</p>
              <ul className="max-h-[50vh] space-y-3 overflow-auto">
                {memoryFiles?.files?.map((f) => (
                  <li key={f.path} className="rounded border border-line bg-panel p-3 text-sm">
                    <div className="font-mono text-accent">{f.path}</div>
                    <div className="mt-1 text-xs text-gray-500">{f.snippet}</div>
                  </li>
                ))}
              </ul>
            </section>
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
