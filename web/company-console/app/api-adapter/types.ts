// ── Companies ──
export interface Company {
  id: string;
  slug: string;
  name: string;           // mapped from display_name
  description?: string | null;
  status: string;
  createdAt: string;
  hsmiiHome?: string | null;
  issueKeyPrefix?: string;
}

// ── Agents ──
export interface Agent {
  id: string;
  companyId?: string;
  name: string;
  role: string;
  title?: string | null;
  status: string;           // "active", "paused", "terminated", etc
  capabilities?: string | null;
  reportsTo?: string | null;
  adapterType?: string | null;
  adapterConfig?: unknown;
  budgetMonthlyCents?: number | null;
  briefing?: string | null;
  sortOrder: number;
  createdAt?: string;
  updatedAt?: string;
}

// ── Issues/Tasks ──
export interface Issue {
  id: string;
  companyId?: string;
  title: string;
  description?: string | null; // mapped from specification
  status: string;              // mapped from state
  priority: number;
  assigneeId?: string | null;  // mapped from owner_persona
  checkedOutBy?: string | null;
  checkedOutUntil?: string | null;
  dueAt?: string | null;
  slaPolicy?: string | null;
  escalateAfter?: string | null;
  statusReason?: string | null;
  decisionMode?: string;
  parentIssueId?: string | null;
  displayNumber?: number | null;
  goalAncestry?: unknown;
  createdAt?: string;
  run?: TaskRun | null;
}

export interface TaskRun {
  runStatus: string;
  toolCalls: number;
  logTail: string;
  finishedAt?: string | null;
  updatedAt?: string;
}

// ── Goals ──
export interface Goal {
  id: string;
  companyId?: string;
  parentGoalId?: string | null;
  title: string;
  description?: string | null;
  status: string;
  createdAt?: string;
}

// ── Activity ──
export interface ActivityEvent {
  id: string;
  actor: string;
  action: string;
  subjectType: string;
  subjectId: string;
  payload?: unknown;
  createdAt: string;
}

// ── Dashboard ──
export interface DashboardSummary {
  companyOsEnabled: boolean;
  agentsEnabled: number;
  tasksInProgress: number;
  trailLines: number;
  memoryFiles: number;
}

// ── Org Chart ──
export interface OrgNode extends Agent {
  directReports: OrgNode[];
}

// ── Console (HSM-specific) ──
export interface TrailEntry {
  [key: string]: unknown;
}

export interface MemoryFile {
  path: string;
  snippet: string;
}

export interface SearchResult {
  q: string;
  trailHits: { index: number; kind: unknown; preview: string }[];
  memoryHits: { path: string; snippet: string }[];
}

// ── Spend ──
export interface SpendSummary {
  companyId: string;
  totalUsd: number;
  byKind: { kind: string; amountUsd: number }[];
}

// ── Policy Rules ──
export interface PolicyRule {
  id: string;
  companyId: string;
  actionType: string;
  riskLevel: string;
  amountMin?: number | null;
  amountMax?: number | null;
  decisionMode: string;
}

// ── Governance ──
export interface GovernanceEvent {
  id: string;
  actor: string;
  action: string;
  subjectType: string;
  subjectId: string;
  payload?: unknown;
  createdAt: string;
}
