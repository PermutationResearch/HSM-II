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
  due_at?: string | null;
  run?: HsmTaskRun | null;
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
