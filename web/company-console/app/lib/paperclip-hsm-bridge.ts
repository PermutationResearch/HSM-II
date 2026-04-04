/**
 * Typed edge layer: HSM Company OS JSON (snake_case) → Paperclip-shaped view models
 * from `api-adapter/types.ts` (camelCase). Use in React Query mappers, SSR, and tests.
 */
import type {
  ActivityEvent,
  Agent,
  Company,
  GovernanceEvent,
  Issue,
  SpendSummary,
  TaskRun,
} from "@/app/api-adapter/types";
import type {
  HsmCompanyAgentRow,
  HsmCompanyRow,
  HsmSpendSummaryRow,
  HsmTaskRow,
  HsmTaskRun,
} from "./hsm-api-types";

/** Governance event row as returned by `GET .../governance/events`. */
export type HsmGovernanceEventRow = {
  id: string;
  actor: string;
  action: string;
  subject_type: string;
  subject_id: string;
  created_at: string;
  payload?: unknown;
};

function mapTaskRun(run: HsmTaskRun | null | undefined): TaskRun | null {
  if (!run) return null;
  return {
    runStatus: run.status,
    toolCalls: run.tool_calls,
    logTail: run.log_tail,
    finishedAt: run.finished_at ?? null,
    updatedAt: run.updated_at ?? undefined,
  };
}

export function hsmCompanyRowToCompany(row: HsmCompanyRow): Company {
  return {
    id: row.id,
    slug: row.slug,
    name: row.display_name,
    description: null,
    status: "active",
    createdAt: row.created_at,
    hsmiiHome: row.hsmii_home ?? null,
    issueKeyPrefix: row.issue_key_prefix,
  };
}

export function hsmTaskRowToIssue(row: HsmTaskRow): Issue {
  return {
    id: row.id,
    companyId: row.company_id,
    title: row.title,
    description: row.specification ?? null,
    status: row.state,
    priority: typeof row.priority === "number" ? row.priority : 0,
    assigneeId: row.owner_persona ?? null,
    checkedOutBy: row.checked_out_by ?? null,
    checkedOutUntil: row.checked_out_until ?? null,
    dueAt: row.due_at ?? null,
    slaPolicy: null,
    escalateAfter: null,
    statusReason: null,
    decisionMode: row.decision_mode,
    parentIssueId: null,
    displayNumber: row.display_number ?? null,
    goalAncestry: undefined,
    createdAt: undefined,
    run: mapTaskRun(row.run ?? null),
  };
}

export function hsmAgentRowToAgent(row: HsmCompanyAgentRow): Agent {
  return {
    id: row.id,
    companyId: row.company_id,
    name: row.name,
    role: row.role,
    title: row.title ?? null,
    status: row.status,
    capabilities: null,
    reportsTo: null,
    adapterType: null,
    adapterConfig: undefined,
    budgetMonthlyCents: row.budget_monthly_cents ?? null,
    briefing: null,
    sortOrder: typeof row.sort_order === "number" ? row.sort_order : 0,
    createdAt: undefined,
    updatedAt: undefined,
  };
}

export function hsmGovernanceRowToGovernanceEvent(row: HsmGovernanceEventRow): GovernanceEvent {
  return {
    id: row.id,
    actor: row.actor,
    action: row.action,
    subjectType: row.subject_type,
    subjectId: row.subject_id,
    payload: row.payload,
    createdAt: row.created_at,
  };
}

/** Alias: activity feed uses the same shape. */
export function hsmGovernanceRowToActivityEvent(row: HsmGovernanceEventRow): ActivityEvent {
  const g = hsmGovernanceRowToGovernanceEvent(row);
  return {
    id: g.id,
    actor: g.actor,
    action: g.action,
    subjectType: g.subjectType,
    subjectId: g.subjectId,
    payload: g.payload,
    createdAt: g.createdAt,
  };
}

export function hsmSpendRowToSpendSummary(row: HsmSpendSummaryRow): SpendSummary {
  return {
    companyId: row.company_id,
    totalUsd: row.total_usd,
    byKind: row.by_kind.map((k) => ({ kind: k.kind, amountUsd: k.amount_usd })),
  };
}
