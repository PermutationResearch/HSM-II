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
  /** Paperclip-style work container */
  project_id?: string | null;
  /** Relative to company `hsmii_home`; JSON array from API */
  workspace_attachment_paths?: unknown;
  /** JSON array of `{ kind, ref }` (skill | sop | tool | pack | agent | mode | label | …) */
  capability_refs?: unknown;
};

export type HsmProjectRow = {
  id: string;
  company_id: string;
  title: string;
  description?: string | null;
  status: string;
  sort_order: number;
  created_at: string;
  updated_at: string;
};

/** Company catalog for task labels (`capability_refs` entries with `kind: "label"`). */
export type HsmIssueLabelRow = {
  id: string;
  company_id: string;
  slug: string;
  display_name: string;
  description?: string | null;
  sort_order: number;
  created_at: string;
  updated_at: string;
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

export type HsmMemoryArtifact = {
  id: string;
  company_id: string;
  memory_id?: string | null;
  media_type: string;
  source_type: string;
  source_uri?: string | null;
  storage_uri?: string | null;
  title?: string | null;
  checksum?: string | null;
  size_bytes?: number | null;
  extraction_status:
    | "queued"
    | "extracting"
    | "chunked"
    | "summarized"
    | "indexed"
    | "retry_waiting"
    | "failed"
    | "dead_letter";
  extraction_provider?: string | null;
  retry_count: number;
  last_error?: string | null;
  document_date?: string | null;
  event_date?: string | null;
  valid_from?: string | null;
  valid_to?: string | null;
  entity_type?: string | null;
  entity_id?: string | null;
  contains_pii: boolean;
  redacted_text?: string | null;
  extracted_text?: string | null;
  metadata?: Record<string, unknown>;
  created_at: string;
  updated_at: string;
};

export type HsmMemoryChunk = {
  id: string;
  artifact_id: string;
  memory_id?: string | null;
  chunk_index: number;
  text: string;
  summary_l0?: string | null;
  summary_l1?: string | null;
  token_count: number;
  modality: string;
  page_number?: number | null;
  time_start_ms?: number | null;
  time_end_ms?: number | null;
  entity_type?: string | null;
  entity_id?: string | null;
  document_date?: string | null;
  event_date?: string | null;
  valid_from?: string | null;
  valid_to?: string | null;
  source_range?: Record<string, unknown>;
  contains_pii: boolean;
  redacted_text?: string | null;
};

export type HsmMemoryMatch = {
  id: string;
  matched_via: string[];
  supporting_chunks: Array<{
    chunk_id: string;
    chunk_index: number;
    text: string;
    modality: string;
    source_label?: string | null;
  }>;
  lineage_summary?: string | null;
  latest_version_only: boolean;
};

export type HsmMemoryInspect = {
  memory: HsmCompanyMemoryEntry & {
    supersedes_memory_id?: string | null;
    is_latest: boolean;
    version: number;
    document_date?: string | null;
    event_date?: string | null;
    valid_from?: string | null;
    valid_to?: string | null;
    entity_type?: string | null;
    entity_id?: string | null;
    source_type?: string | null;
    source_uri?: string | null;
    chunk_id?: string | null;
    source_range?: Record<string, unknown> | null;
    contains_pii: boolean;
    redacted_body?: string | null;
    primary_artifact_id?: string | null;
    source_artifact_count: number;
    chunk_count: number;
  };
  artifacts: HsmMemoryArtifact[];
  chunks: HsmMemoryChunk[];
  lineage: Array<{
    id: string;
    version: number;
    is_latest: boolean;
    supersedes_memory_id?: string | null;
  }>;
};

export type HsmCompanyAgentRow = {
  id: string;
  company_id: string;
  name: string;
  role: string;
  title?: string | null;
  capabilities?: string | null;
  reports_to?: string | null;
  adapter_type?: string | null;
  adapter_config?: unknown;
  briefing?: string | null;
  status: string;
  budget_monthly_cents?: number | null;
  sort_order?: number;
};

export type HsmCompanyCredential = {
  id: string;
  company_id: string;
  provider_key: string;
  label: string;
  env_var?: string | null;
  masked_preview: string;
  notes?: string | null;
  status: "connected" | "missing" | "error";
  metadata?: Record<string, unknown>;
  created_at: string;
  updated_at: string;
};

export type HsmSkillBankEntry = {
  id?: string;
  company_id?: string;
  slug: string;
  name: string;
  description: string;
  body?: string;
  skill_path?: string;
  source?: string;
  updated_at?: string;
  linked_agents?: string[];
  linked_agent_count?: number;
  company_count?: number;
  company_names?: string[];
};

export type HsmBrowserProviderStatus = {
  key: string;
  label: string;
  kind: string;
  configured: boolean;
  credential_preview?: string | null;
  api_base: string;
  prompt_cache_enabled?: boolean;
  thinking_prefill_enabled?: boolean;
};

export type HsmThreadSession = {
  id: string;
  company_id: string;
  session_key: string;
  title: string;
  participants: string[];
  state: Record<string, unknown>;
  is_active: boolean;
  created_by?: string | null;
  created_at: string;
  updated_at: string;
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

/** `intelligence_signals` table row */
export type HsmSignalRow = {
  id: string;
  kind: string;
  description: string;
  severity: number;
  composition_success: boolean | null;
  escalated_to: string | null;
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
  signals?: {
    recent: HsmSignalRow[];
    by_kind_7d: Record<string, number>;
  };
  error?: string;
};

/** `GET /api/company/companies/:id/self-improvement/summary` */
export type HsmSelfImprovementSummary = {
  total_failures_7d: number;
  repeat_failure_rate_7d: number;
  first_pass_success_rate_7d: number;
  proposals_created_7d: number;
  proposals_applied_7d: number;
  rollback_rate_7d: number;
  avg_recovery_hours_7d?: number | null;
};

export type HsmSelfImprovementProposal = {
  id: string;
  failure_event_id?: string | null;
  proposal_type: string;
  target_surface: string;
  patch_kind: string;
  rationale: string;
  status:
    | "proposed"
    | "replay_passed"
    | "replay_failed"
    | "applied"
    | "rejected"
    | "rolled_back";
  auto_apply_eligible: boolean;
  replay_passed?: boolean | null;
  replay_report?: Record<string, unknown> | null;
  applied_at?: string | null;
  created_at: string;
};

/** `GET .../promotions` — store promotion audit row */
export type HsmStorePromotion = {
  id: string;
  company_id: string;
  source_store: "roodb" | "ladybug" | "sqlite";
  source_id: string;
  source_snapshot: Record<string, unknown>;
  target_table: string;
  target_id?: string | null;
  promoted_by: string;
  status: "promoted" | "rolled_back" | "superseded";
  created_at: string;
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
  profile?: HsmCompanyProfile | null;
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
  roi?: {
    avg_cycle_time_hours_30d: number;
    manual_interventions_per_task_7d: number;
    retries_per_task_7d: number;
    tasks_closed_per_day_14d: number;
    tasks_created_7d: number;
    manual_interventions_7d: number;
    retries_7d: number;
  };
  universality?: {
    profile_size_tier: string;
    profile_business_model: string;
    time_to_first_value_hours?: number | null;
    setup_completion_rate: number;
    template_adoption_events_30d: number;
    cost_per_resolved_operation: number;
  };
  integration_status: Record<string, unknown>;
};

export type HsmCompanyConnector = {
  id: string;
  company_id: string;
  connector_key: string;
  label: string;
  provider_key: string;
  base_url?: string | null;
  auth_mode: string;
  credential_provider_key?: string | null;
  policy: Record<string, unknown>;
  status: string;
  last_success_at?: string | null;
  last_failure_at?: string | null;
  last_error?: string | null;
  metadata: Record<string, unknown>;
  created_at: string;
  updated_at: string;
};

export type HsmConnectorTemplate = {
  key: string;
  label: string;
  category: string;
  provider_key: string;
  auth_mode: string;
  recommendation?: "must_have" | "optional" | "deferred" | string;
};

export type HsmEmailOperatorQueueItem = {
  id: string;
  company_id: string;
  connector_key?: string | null;
  mailbox: string;
  thread_id?: string | null;
  message_id?: string | null;
  from_address: string;
  subject: string;
  body_text: string;
  suggested_reply?: string | null;
  suggested_by_agent?: string | null;
  status: string;
  owner_decision?: string | null;
  decided_by?: string | null;
  decided_at?: string | null;
  sent_at?: string | null;
  metadata: Record<string, unknown>;
  created_at: string;
  updated_at: string;
};

export type HsmCompanyProfile = {
  company_id: string;
  industry: string;
  business_model: string;
  channel_mix: unknown;
  compliance_level: string;
  size_tier: "solo" | "team" | "org" | string;
  inferred: boolean;
  profile_source: string;
  metadata: Record<string, unknown>;
  created_at: string;
  updated_at: string;
};

export type HsmWorkflowPack = {
  key: string;
  label: string;
  default_risk: "low" | "medium" | "high" | string;
  automation_limit: string;
};

export type HsmOperatorInbox = {
  company_id: string;
  profile: HsmCompanyProfile;
  lanes: Array<{
    id: string;
    label: string;
    item_kinds: string[];
    sla?: string;
  }>;
  counts: {
    tasks: number;
    emails: number;
    failures: number;
    total: number;
  };
  items: Array<{
    kind: "task" | "email" | "failure" | string;
    id: string;
    title: string;
    state: string;
    priority: number;
    mailbox?: string;
    from_address?: string;
    body_text?: string;
    suggested_reply?: string | null;
    confidence?: number;
    [key: string]: unknown;
  }>;
};
