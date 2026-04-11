export type RunStatus = "running" | "success" | "error" | "cancelled";
export type ExecutionMode = "worker" | "llm_simulated" | "pending" | "unknown";
export type RunLoopState =
  | "running"
  | "paused_auth"
  | "paused_approval"
  | "resumed"
  | "completed"
  | "cancelled";

export type CheckpointKind = "approval" | "auth";

export type PendingApprovalCheckpoint = {
  tool_name?: string;
  call_id?: string | null;
  message?: string;
  approval_key?: string | null;
  execution_id?: string | null;
  kind?: CheckpointKind;
  ts_ms?: number;
};

export type AgentRunMeta = {
  execution_mode?: ExecutionMode | string;
  loop_state?: RunLoopState | string;
  needs_human?: boolean;
  pending_approval_checkpoint?: PendingApprovalCheckpoint | null;
  [k: string]: unknown;
};

export type AgentRunRecord = {
  id?: string;
  status?: string;
  summary?: string | null;
  task_id?: string | null;
  meta?: AgentRunMeta;
};

export type AgentRunResponse = { run?: AgentRunRecord | null; error?: string };
export type CompanyTasksResponse = { tasks?: unknown[]; error?: string };
export type AgentChatReplyResponse = {
  ok?: boolean;
  reply?: string;
  at?: string;
  context_notes?: unknown;
  error?: string;
  run_id?: string;
  skill?: string;
};
export type ResumeRunResponse = {
  ok?: boolean;
  error?: string;
  run_id?: string;
  status?: string;
  execution_mode?: string;
};

export const ALLOWED_LOOP_TRANSITIONS: Record<RunLoopState, RunLoopState[]> = {
  running: ["paused_auth", "paused_approval", "completed", "cancelled"],
  paused_auth: ["resumed", "cancelled"],
  paused_approval: ["resumed", "cancelled"],
  resumed: ["running", "completed", "cancelled"],
  completed: [],
  cancelled: [],
};

export function canTransitionRunLoopState(from: RunLoopState, to: RunLoopState): boolean {
  return ALLOWED_LOOP_TRANSITIONS[from]?.includes(to) ?? false;
}

export function parseExecutionMode(input: unknown): ExecutionMode {
  const v = typeof input === "string" ? input : "";
  if (v === "worker" || v === "llm_simulated" || v === "pending") return v;
  return "unknown";
}

export function parseRunStatus(input: unknown): RunStatus {
  const v = typeof input === "string" ? input : "";
  if (v === "running" || v === "success" || v === "error" || v === "cancelled") return v;
  return "running";
}

export function parseRunLoopState(input: unknown): RunLoopState | null {
  const v = typeof input === "string" ? input : "";
  if (
    v === "running" ||
    v === "paused_auth" ||
    v === "paused_approval" ||
    v === "resumed" ||
    v === "completed" ||
    v === "cancelled"
  ) {
    return v;
  }
  return null;
}

export function asObject(input: unknown): Record<string, unknown> | null {
  if (!input || typeof input !== "object" || Array.isArray(input)) return null;
  return input as Record<string, unknown>;
}

export function asArray(input: unknown): unknown[] {
  return Array.isArray(input) ? input : [];
}

