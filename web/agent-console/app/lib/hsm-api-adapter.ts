/**
 * Paperclip-shaped view models for UI that was ported from Paperclip patterns.
 * Source of truth remains HSM API types — map at the edge only.
 */
import type { HsmCompanyRow, HsmCompanyAgentRow, HsmTaskRow } from "./hsm-api-types";

export type PcCompany = {
  id: string;
  slug: string;
  displayName: string;
  issueKeyPrefix: string;
};

export type PcIssue = {
  id: string;
  identifier: string;
  title: string;
  status: string;
  assigneeId: string | null;
  priority: number;
  decisionMode?: string;
};

export type PcWorkforceAgent = {
  id: string;
  name: string;
  role: string;
  title: string | null;
  status: string;
  budgetMonthlyCents: number | null;
};

export function toPcCompany(row: HsmCompanyRow): PcCompany {
  return {
    id: row.id,
    slug: row.slug,
    displayName: row.display_name,
    issueKeyPrefix: (row.issue_key_prefix ?? "HSM").toUpperCase(),
  };
}

export function taskToPcIssue(task: HsmTaskRow, issueKeyPrefix: string): PcIssue {
  const p = issueKeyPrefix.toUpperCase();
  const identifier =
    typeof task.display_number === "number" ? `${p}-${task.display_number}` : `HSM-${task.id.slice(0, 8)}`;
  return {
    id: task.id,
    identifier,
    title: task.title,
    status: task.state,
    assigneeId: task.owner_persona ?? task.checked_out_by ?? null,
    priority: typeof task.priority === "number" ? task.priority : 0,
    decisionMode: task.decision_mode,
  };
}

export function toPcWorkforceAgent(row: HsmCompanyAgentRow): PcWorkforceAgent {
  return {
    id: row.id,
    name: row.name,
    role: row.role,
    title: row.title ?? null,
    status: row.status,
    budgetMonthlyCents: row.budget_monthly_cents ?? null,
  };
}

/** Full Paperclip-shaped models (`api-adapter/types`) from HSM rows — use for typed clients and tests. */
export {
  hsmAgentRowToAgent,
  hsmCompanyRowToCompany,
  hsmGovernanceRowToActivityEvent,
  hsmGovernanceRowToGovernanceEvent,
  hsmSpendRowToSpendSummary,
  hsmTaskRowToIssue,
  type HsmGovernanceEventRow,
} from "./paperclip-hsm-bridge";
