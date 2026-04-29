// ─── Request/Response ───────────────────────────────────────────────────────

export interface RunRequest {
  task_id: string;
  prompt: string;
  /** Working directory for the claude subprocess (defaults to cwd of this process). */
  cwd?: string;
  /** Maximum agent turns (forwarded as --max-turns). */
  max_turns?: number;
  /**
   * Claude Code tool names (capitalised: "Read", "Edit", "Bash", ...).
   * If absent or empty, all tools are allowed.
   */
  allowed_tools?: string[];
  /** Extra env vars injected into the subprocess environment. */
  env?: Record<string, string>;
  /** Optional harness resume token from prior waiting interaction/checkpoint. */
  resume_token?: string;
  /** Optional checkpoint id to annotate replay/resume runs. */
  checkpoint_ref?: string;
}

// ─── CompletionEvent (mirrors Rust runtime_control::CompletionEvent) ─────────

export interface CompletionEvent {
  event_type: string;
  task_key?: string;
  tool_name?: string;
  call_id?: string;
  success: boolean;
  message: string;
  ts_ms: number;
  input?: unknown;
  stream_event?: unknown;
  output_len?: number;
  /** Canonical HarnessV1 state emitted by claude-harness. */
  harness_state?: HarnessState;
  /** Blocking interaction kind when waiting on external input. */
  interaction_kind?: HarnessInteractionKind;
  /** Opaque token used for resume / elicitation callbacks. */
  resume_token?: string;
  /** Checkpoint reference id for persisted snapshots. */
  checkpoint_ref?: string;
  /** Structured interaction payload (approval/elicitation/tool metadata). */
  interaction?: Record<string, unknown>;
}

export type HarnessState =
  | 'queued'
  | 'running'
  | 'waiting_tool'
  | 'waiting_elicitation'
  | 'waiting_approval'
  | 'paused'
  | 'checkpointed'
  | 'resuming'
  | 'completed'
  | 'cancelled'
  | 'failed';

export type HarnessInteractionKind =
  | 'tool_call'
  | 'elicitation'
  | 'approval'
  | 'operator_input'
  | 'subagent_task';

// ─── Claude -p --output-format stream-json NDJSON types ─────────────────────

export type ClaudeStreamLine =
  | SystemInitMessage
  | AssistantMessage
  | UserMessage
  | StreamEventMessage
  | ResultSuccessMessage
  | ResultErrorMessage;

export interface SystemInitMessage {
  type: 'system';
  subtype: 'init';
  tools: string[];
  model: string;
  cwd: string;
  session_id: string;
}

export interface TextBlock {
  type: 'text';
  text: string;
}

export interface ToolUseBlock {
  type: 'tool_use';
  id: string;
  name: string;
  input: Record<string, unknown>;
}

export interface ThinkingBlock {
  type: 'thinking';
  thinking: string;
}

export type ContentBlock = TextBlock | ToolUseBlock | ThinkingBlock;

export interface AssistantMessage {
  type: 'assistant';
  message: {
    role: 'assistant';
    content: ContentBlock[];
  };
  session_id?: string;
}

export interface ToolResultBlock {
  type: 'tool_result';
  tool_use_id: string;
  content: Array<{ type: 'text'; text: string }> | string;
  is_error?: boolean;
}

export interface UserMessage {
  type: 'user';
  message: {
    role: 'user';
    content: ToolResultBlock[];
  };
  session_id?: string;
}

export interface StreamEventMessage {
  type: 'stream_event';
  event: unknown;
  parent_tool_use_id?: string | null;
}

export interface ResultSuccessMessage {
  type: 'result';
  subtype: 'success';
  result: string;
  duration_ms: number;
  duration_api_ms: number;
  num_turns: number;
  total_cost_usd: number;
  session_id: string;
  is_error: boolean;
}

export interface ResultErrorMessage {
  type: 'result';
  subtype:
    | 'error_during_execution'
    | 'error_max_turns'
    | 'error_max_budget_usd'
    | 'error_max_structured_output_retries';
  errors: string[];
  duration_ms: number;
  num_turns: number;
  total_cost_usd: number;
  session_id: string;
  is_error: boolean;
}
