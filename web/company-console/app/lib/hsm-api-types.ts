/** Raw JSON shapes from `hsm_console` Company OS (keep in sync with Rust handlers). */

export type HsmCompanyHealth = {
  postgres_configured?: boolean;
  postgres_ok?: boolean;
};

export type HsmCompanyRow = {
  id: string;
  slug: string;
  display_name: string;
  hsmii_home?: string | null;
  issue_key_prefix?: string;
  /** Company-wide Markdown for LLM context (PATCH company). */
  context_markdown?: string | null;
  created_at: string;
};

export type HsmTaskRun = {
  status: string;
  tool_calls: number;
  log_tail: string;
  finished_at?: string | null;
  updated_at?: string | null;
};

export type HsmTaskRow = {
  id: string;
  company_id?: string;
  title: string;
  state: string;
  specification?: string | null;
  owner_persona?: string | null;
  checked_out_by?: string | null;
  checked_out_until?: string | null;
  priority?: number;
  display_number?: number | null;
  decision_mode?: string;
  /** Surfaces in human inbox (agent escalation) */
  requires_human?: boolean;
  due_at?: string | null;
  run?: HsmTaskRun | null;
  /** Relative to company `hsmii_home`; JSON array from API */
  workspace_attachment_paths?: unknown;
  /** JSON array of `{ kind, ref }` (skill | sop | tool | pack | agent) */
  capability_refs?: unknown;
};

/** Postgres-backed shared/agent memory entries */
export type HsmCompanyMemoryEntry = {
  id: string;
  company_id: string;
  scope: string;
  company_agent_id?: string | null;
  title: string;
  body: string;
  tags: string[];
  source: string;
  summary_l0?: string | null;
  summary_l1?: string | null;
  created_at: string;
  updated_at: string;
};

export type HsmCompanyAgentRow = {
  id: string;
  company_id: string;
  name: string;
  role: string;
  title?: string | null;
  status: string;
  budget_monthly_cents?: number | null;
  sort_order?: number;
};

/** `GET /api/company/companies/:id/spend/summary` */
export type HsmSpendSummaryRow = {
  company_id: string;
  total_usd: number;
  by_kind: { kind: string; amount_usd: number }[];
};

/** `GET /api/company/companies/:id/goals` row */
export type HsmGoalRow = {
  id: string;
  company_id: string;
  parent_goal_id: string | null;
  title: string;
  description: string | null;
  status: string;
  created_at: string;
};

/** `governance_events` rows returned in intelligence summary workflow_feed */
export type HsmWorkflowFeedEvent = {
  id: string;
  actor: string;
  action: string;
  subject_type: string;
  subject_id: string;
  payload: Record<string, unknown>;
  created_at: string;
};

/** `GET /api/company/companies/:id/intelligence/summary` */
export type HsmIntelligenceSummary = {
  company_id: string;
  source: string;
  goals: { total: number; active: number };
  tasks: {
    total: number;
    open: number;
    in_progress: number;
    done_or_closed: number;
    requires_human_open: number;
    checked_out_now: number;
  };
  workforce: { agents_non_terminated: number };
  spend: { total_usd: number };
  workflow_feed: HsmWorkflowFeedEvent[];
  error?: string;
};

/** `GET .../agents/:agentId/inventory` — company skill row without body */
export type HsmCompanySkillSummary = {
  id: string;
  slug: string;
  name: string;
  description: string;
  skill_path: string;
  source: string;
  updated_at: string;
  on_agent_roster?: boolean;
};

export type HsmAgentInventoryInstructionFile = {
  path: string;
  name: string;
  size_bytes: number;
  modified_at?: string | null;
};

/** `GET /api/company/companies/:id/agents/:agentId/inventory` */
export type HsmAgentInventory = {
  agent: {
    id: string;
    company_id: string;
    name: string;
    role: string;
    title?: string | null;
    capabilities?: string | null;
    adapter_type?: string | null;
    adapter_config?: unknown;
    briefing_preview?: string | null;
  };
  roster_skill_refs: string[];
  skills_linked: { ref: string; skill: HsmCompanySkillSummary }[];
  unresolved_skill_refs: string[];
  company_skills_catalog: HsmCompanySkillSummary[];
  instruction_markdown_files: HsmAgentInventoryInstructionFile[];
  hsmii_home_configured: boolean;
};

/** `GET /api/company/companies/:id/ops/overview` */
export type HsmCompanyOpsOverview = {
  company: HsmCompanyRow;
  ops_config: {
    loaded: boolean;
    path?: string | null;
    error?: string | null;
    summary?: unknown;
  };
  overview: {
    goals_total: number;
    tasks_total: number;
    tasks_open: number;
    tasks_requires_human: number;
    agents_total: number;
    spend_total_usd: number;
    month: string;
  };
  budgets: Array<{
    id: string;
    scope: "company" | "role" | string;
    role_id?: string | null;
    kind: string;
    cap_monthly: number;
    hard_stop: boolean;
    usage_usd?: number | null;
    utilization?: number | null;
    over_cap?: boolean | null;
    enforcement_ready?: boolean;
  }>;
  heartbeats?: {
    configured?: unknown;
    runtime_state?: unknown;
  };
  tickets?: unknown;
  ticket_sync?: {
    configured_tickets?: number;
    created?: number;
    updated?: number;
    skipped?: boolean;
    reason?: string;
    error?: string;
  };
  org?: unknown;
  governance_recent: HsmWorkflowFeedEvent[];
  spend: {
    total_usd: number;
    by_kind: Array<{ kind: string; amount_usd: number }>;
    by_agent_ref: Array<{ agent_ref: string; amount_usd: number }>;
  };
  audit: {
    path?: string;
    turns?: number;
    avg_tool_prompt_tokens?: number;
    avg_skill_prompt_tokens?: number;
    avg_exposed_tools?: number;
    avg_hidden_tools?: number;
    error?: string;
  };
  integration_status: Record<string, unknown>;
};
