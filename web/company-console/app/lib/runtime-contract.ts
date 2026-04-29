export type RunStatus = "running" | "success" | "error" | "cancelled";
export type ExecutionMode = "worker" | "llm_simulated" | "pending" | "unknown";
export type RunLoopState =
  | "running"
  | "waiting_tool"
  | "waiting_elicitation"
  | "waiting_approval"
  | "checkpointed"
  | "resuming"
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

export type PendingElicitationCheckpoint = {
  tool_name?: string;
  call_id?: string | null;
  message?: string;
  resume_token?: string | null;
  interaction?: Record<string, unknown> | null;
  ts_ms?: number;
};

export type AgentRunMeta = {
  execution_mode?: ExecutionMode | string;
  loop_state?: RunLoopState | string;
  needs_human?: boolean;
  pending_approval_checkpoint?: PendingApprovalCheckpoint | null;
  pending_elicitation_checkpoint?: PendingElicitationCheckpoint | null;
  pending_interactions?: Array<{
    kind?: "approval" | "elicitation";
    resume_token?: string;
    tool_name?: string;
    call_id?: string | null;
    message?: string;
    interaction?: Record<string, unknown> | null;
    ts_ms?: number;
  }>;
  todo_queue?: Array<{
    id?: string;
    content?: string;
    status?: "pending" | "in_progress" | "completed" | "cancelled" | string;
    updated_at?: string;
  }>;
  subagent_tasks?: Array<{
    id?: string;
    description?: string;
    subagent_type?: string;
    model?: string;
    status?: "running" | "completed" | "failed" | string;
    updated_at?: string;
  }>;
  bridge_proxy?: {
    mode?: "local" | "proxy" | string;
    status?: "idle" | "active" | "error" | string;
    last_tool?: string | null;
    last_mcp_server?: string | null;
    last_mcp_tool?: string | null;
    last_error?: string | null;
    call_count?: number;
    updated_at?: string;
  } | null;
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
  running: [
    "waiting_tool",
    "waiting_elicitation",
    "waiting_approval",
    "paused_auth",
    "paused_approval",
    "checkpointed",
    "completed",
    "cancelled",
  ],
  waiting_tool: ["running", "checkpointed", "resuming", "resumed", "cancelled"],
  waiting_elicitation: ["checkpointed", "resuming", "resumed", "cancelled"],
  waiting_approval: ["checkpointed", "resuming", "resumed", "cancelled"],
  checkpointed: ["resuming", "resumed", "running", "cancelled"],
  resuming: ["running", "resumed", "completed", "cancelled"],
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
    v === "waiting_tool" ||
    v === "waiting_elicitation" ||
    v === "waiting_approval" ||
    v === "checkpointed" ||
    v === "resuming" ||
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

