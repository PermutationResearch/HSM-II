/**
 * Server-side NDJSON stream for operator chat: proxies Company OS runtime SSE (tool events)
 * while worker dispatch runs, and emits Claude Code–compatible `stream_event` lines
 * (Anthropic Messages API shapes: message_start, content_block_delta, message_stop) on the
 * OpenRouter streaming path.
 *
 * Runtime telemetry is emitted strictly as `{ type: "runtime", payload }` NDJSON lines.
 * Only genuine model stream events are sent as Claude SDK-compatible `{ type: "stream_event", event }`.
 *
 * When dispatch still goes to the worker (skills / execution / no key), an optional **companion**
 * OpenRouter stream can run in parallel (serialized with runtime SSE) so the rail shows live
 * `stream_event` text while tools run. Companion is OFF by default; set
 * `HSM_WORKER_COMPANION_STREAM=1` to enable. You can force-disable with
 * `HSM_DISABLE_WORKER_COMPANION_STREAM=1`.
 * If enabled, set `HSM_WORKER_COMPANION_WITH_CODING=0` to skip sidecar narration on coding /
 * execution turns.
 */

import {
  buildCompactedContextBundle,
  buildStrictToolFlowTrace,
  buildSystemPrompt,
  companyOsHarnessAddendum,
  compactNotesForLlm,
  deriveToolExecutionPolicy,
  detectSkillDispatch,
  dispatchSkillToWorker,
  finalizeAgentRun,
  createAgentRun,
  looksLikeImplicitWorkspacePointer,
  looksLikeQuickToolIntent,
  looksLikeRepoInfoQuestion,
  looksLikeSkillsOrCatalogQuestion,
  operatorChatQuickToolPromptMode,
  executeTaskAction,
  operatorChatShouldRouteWorker,
  parseOptimizeCommand,
  parseSentinelReputationCommand,
  parseTaskManagementAction,
  querySentinelReputation,
  readAgentChatBackend,
  resolveAgentForPersona,
  saveCompactionToMemory,
  isThinHarnessModel,
  type WorkerDispatchResult,
  UPSTREAM,
  upsertThreadSessionState,
} from "@/app/lib/agent-chat-server";
import {
  anthropicContentBlockStartText,
  anthropicContentBlockStop,
  anthropicMessageDelta,
  anthropicMessageStart,
  anthropicMessageStop,
  anthropicTextDelta,
  wrapSdkStreamEvent,
} from "@/app/lib/claude-stream-shape";
import {
  buildExecutionEvidenceReply,
  shouldExposeRuntimePayload,
} from "@/app/lib/agent-chat-stream-policy.mjs";
import { openRouterStreamDeltaToText, normalizeChatCompletionMessageContent } from "@/app/lib/llm-text-content";
import { asObject } from "@/app/lib/runtime-contract";
import { evaluateStrictActionPolicy } from "@/app/lib/strict-security";

export type AgentChatRequestBody = {
  taskId: string;
  persona: string;
  companyId?: string;
  title?: string;
  role?: string;
  notes: Array<{ at: string; actor: string; text: string }>;
};

export type NdjsonLineWriter = (obj: Record<string, unknown>) => Promise<void>;
type RuntimePayloadHandler = (payload: Record<string, unknown>) => void | Promise<void>;
type ChatBackend = NonNullable<ReturnType<typeof readAgentChatBackend>>;

type MutableRunArtifact = {
  path: string;
  callCount: number;
  tools: Set<string>;
  lastTool: string;
  beforeSnapshot: string | null;
  afterSnapshot: string | null;
  updatedAt: string;
};

type RunArtifactsPayload = {
  version: number;
  generated_at: string;
  touched_files: Array<{
    path: string;
    call_count: number;
    tools: string[];
    last_tool: string;
    before_snapshot: string | null;
    after_snapshot: string | null;
    updated_at: string;
  }>;
};

type OperationalTodoItem = {
  id: string;
  content: string;
  status: "pending" | "in_progress" | "completed" | "cancelled";
  updated_at: string;
};

type OperationalSubagentTask = {
  id: string;
  description: string;
  subagent_type?: string;
  model?: string;
  status: "running" | "completed" | "failed";
  updated_at: string;
};

type BridgeProxyState = {
  mode: "local" | "proxy";
  status: "idle" | "active" | "error";
  last_tool?: string | null;
  last_mcp_server?: string | null;
  last_mcp_tool?: string | null;
  last_error?: string | null;
  call_count: number;
  updated_at: string;
};

type OperationalStateAccumulator = {
  todo_queue: Map<string, OperationalTodoItem>;
  subagent_tasks: Map<string, OperationalSubagentTask>;
  bridge_proxy: BridgeProxyState;
};

const OPERATOR_CHAT_TELEMETRY_WAIT_EXEC_MS = Math.min(
  Math.max(Number.parseInt(process.env.HSM_OPERATOR_CHAT_TELEMETRY_WAIT_EXEC_MS ?? "120000", 10) || 120000, 5000),
  300_000,
);
const OPERATOR_CHAT_TELEMETRY_WAIT_ANALYSIS_MS = Math.min(
  Math.max(Number.parseInt(process.env.HSM_OPERATOR_CHAT_TELEMETRY_WAIT_ANALYSIS_MS ?? "30000", 10) || 30000, 5000),
  300_000,
);
// Build-heavy skills (cargo check, npm build) can take 2-3 min.
const BUILD_HEAVY_SKILLS = new Set(["validate-delivery", "perf-analyzer", "orchestrate-review"]);

type AgentChatSlashCommand =
  | { kind: "skill"; skillSlug: string }
  | { kind: "help" };

function parseAgentChatSlashCommand(text: string, knownSlugs: string[]): AgentChatSlashCommand | null {
  const t = text.trim();
  if (!t.startsWith("/")) return null;
  const body = t.slice(1).trim();
  if (!body) return { kind: "help" };
  const [cmdRaw, ...rest] = body.split(/\s+/);
  const cmd = cmdRaw.toLowerCase();
  if (cmd === "help") return { kind: "help" };
  if (cmd === "skill" && rest.length > 0) {
    const slug = rest.join("-").toLowerCase();
    const matched = knownSlugs.find((s) => s.toLowerCase() === slug) ?? slug;
    return { kind: "skill", skillSlug: matched };
  }
  const slashAsSkill = knownSlugs.find((s) => s.toLowerCase() === cmd);
  return slashAsSkill ? { kind: "skill", skillSlug: slashAsSkill } : null;
}

function slashCommandWorkerInstruction(cmd: AgentChatSlashCommand): string {
  if (cmd.kind === "help") {
    return "List available slash commands and supported agent-chat skills.";
  }
  return `Run the \`${cmd.skillSlug}\` skill in the worker agent loop and report results.`;
}

function makeOperationalAccumulator(): OperationalStateAccumulator {
  return {
    todo_queue: new Map<string, OperationalTodoItem>(),
    subagent_tasks: new Map<string, OperationalSubagentTask>(),
    bridge_proxy: {
      mode: "local",
      status: "idle",
      call_count: 0,
      updated_at: new Date().toISOString(),
      last_tool: null,
      last_mcp_server: null,
      last_mcp_tool: null,
      last_error: null,
    },
  };
}

function captureOperationalStateFromRuntime(payload: Record<string, unknown>, acc: OperationalStateAccumulator): void {
  const eventType = typeof payload.event_type === "string" ? payload.event_type : "";
  const toolName = typeof payload.tool_name === "string" ? payload.tool_name : "";
  const now = new Date().toISOString();
  if (eventType === "tool_start" || eventType === "tool_complete" || eventType === "tool_error") {
    acc.bridge_proxy.call_count += 1;
    acc.bridge_proxy.last_tool = toolName || acc.bridge_proxy.last_tool;
    acc.bridge_proxy.status = eventType === "tool_error" ? "error" : "active";
    acc.bridge_proxy.updated_at = now;
  }
  const input = asObject(payload.input);
  if (toolName === "TodoWrite" && input && Array.isArray(input.todos)) {
    for (const item of input.todos) {
      if (!item || typeof item !== "object") continue;
      const id = typeof (item as { id?: unknown }).id === "string" ? String((item as { id: unknown }).id) : "";
      if (!id) continue;
      acc.todo_queue.set(id, {
        id,
        content: typeof (item as { content?: unknown }).content === "string" ? String((item as { content: unknown }).content) : "",
        status:
          typeof (item as { status?: unknown }).status === "string"
            ? ((item as { status: unknown }).status as OperationalTodoItem["status"])
            : "pending",
        updated_at: now,
      });
    }
  }
  if (toolName === "Subagent") {
    const id =
      (typeof payload.call_id === "string" && payload.call_id) ||
      (typeof payload.task_key === "string" && payload.task_key) ||
      `subagent-${acc.subagent_tasks.size + 1}`;
    acc.subagent_tasks.set(id, {
      id,
      description: typeof input?.prompt === "string" ? input.prompt : "Subagent task",
      subagent_type: typeof input?.subagent_type === "string" ? input.subagent_type : undefined,
      model: typeof input?.model === "string" ? input.model : undefined,
      status: eventType === "tool_error" ? "failed" : eventType === "tool_complete" ? "completed" : "running",
      updated_at: now,
    });
  }
  if (toolName === "CallMcpTool") {
    acc.bridge_proxy.mode = "proxy";
    acc.bridge_proxy.last_mcp_server = typeof input?.server === "string" ? input.server : acc.bridge_proxy.last_mcp_server;
    acc.bridge_proxy.last_mcp_tool =
      typeof input?.toolName === "string" ? input.toolName : acc.bridge_proxy.last_mcp_tool;
    acc.bridge_proxy.updated_at = now;
  }
  if (eventType === "tool_error") {
    acc.bridge_proxy.last_error =
      typeof payload.message === "string" ? payload.message : acc.bridge_proxy.last_error;
  }
}

const ARTIFACT_SNAPSHOT_MAX_CHARS = 800;

function truncateSnapshot(input: unknown): string | null {
  if (typeof input !== "string") return null;
  const trimmed = input.trim();
  if (!trimmed) return null;
  return trimmed.length > ARTIFACT_SNAPSHOT_MAX_CHARS
    ? `${trimmed.slice(0, ARTIFACT_SNAPSHOT_MAX_CHARS)}…`
    : trimmed;
}

function extractPathFromInput(input: Record<string, unknown> | null): string | null {
  if (!input) return null;
  const raw = typeof input.path === "string" ? input.path : typeof input.file_path === "string" ? input.file_path : "";
  const p = raw.trim().replace(/^\.\/+/, "");
  return p.length > 0 ? p : null;
}

function captureRunArtifactFromRuntime(payload: Record<string, unknown>, bag: Map<string, MutableRunArtifact>) {
  const eventType = typeof payload.event_type === "string" ? payload.event_type : "";
  if (eventType !== "tool_start") return;
  const toolName = typeof payload.tool_name === "string" ? payload.tool_name.trim() : "";
  if (!toolName) return;
  const input = asObject(payload.input);
  const path = extractPathFromInput(input);
  if (!path) return;
  const now = new Date().toISOString();
  const existing = bag.get(path);
  const rec: MutableRunArtifact =
    existing ??
    ({
      path,
      callCount: 0,
      tools: new Set<string>(),
      lastTool: toolName,
      beforeSnapshot: null,
      afterSnapshot: null,
      updatedAt: now,
    } satisfies MutableRunArtifact);

  rec.callCount += 1;
  rec.tools.add(toolName);
  rec.lastTool = toolName;
  rec.updatedAt = now;

  if (toolName === "edit") {
    rec.beforeSnapshot =
      truncateSnapshot(input?.oldText) ?? truncateSnapshot(input?.old_string) ?? rec.beforeSnapshot;
    rec.afterSnapshot =
      truncateSnapshot(input?.newText) ?? truncateSnapshot(input?.new_string) ?? rec.afterSnapshot;
  } else if (toolName === "write") {
    rec.afterSnapshot = truncateSnapshot(input?.content) ?? rec.afterSnapshot;
  } else if (toolName === "delete") {
    rec.afterSnapshot = "";
  }

  bag.set(path, rec);
}

function serializeRunArtifacts(bag: Map<string, MutableRunArtifact>): RunArtifactsPayload | null {
  if (bag.size === 0) return null;
  const touched = [...bag.values()]
    .sort((a, b) => a.path.localeCompare(b.path))
    .map((x) => ({
      path: x.path,
      call_count: x.callCount,
      tools: [...x.tools].sort(),
      last_tool: x.lastTool,
      before_snapshot: x.beforeSnapshot,
      after_snapshot: x.afterSnapshot,
      updated_at: x.updatedAt,
    }));
  return {
    version: 1,
    generated_at: new Date().toISOString(),
    touched_files: touched,
  };
}

async function patchRunArtifactsMeta(
  companyId: string,
  runId: string | null | undefined,
  artifacts: RunArtifactsPayload | null,
): Promise<void> {
  if (!runId || !artifacts) return;
  try {
    const runRes = await fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${runId}`);
    if (!runRes.ok) return;
    const runJson = (await runRes.json().catch(() => ({}))) as { run?: { meta?: Record<string, unknown> } };
    const currentMeta = asObject(runJson.run?.meta) ?? {};
    const mergedMeta: Record<string, unknown> = {
      ...currentMeta,
      run_artifacts: artifacts,
    };
    await fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${runId}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ meta: mergedMeta }),
    });
  } catch {
    // Best-effort only.
  }
}

function maybeExecutionReply(params: {
  executionIntent: boolean;
  codingIntent: boolean;
  result: Extract<WorkerDispatchResult, { ok: true }>;
  artifacts: RunArtifactsPayload | null;
}): string | undefined {
  if (!params.executionIntent && !params.codingIntent) return undefined;
  if (!params.result.finalized) return undefined;
  if (params.result.status !== "success") return undefined;
  return buildExecutionEvidenceReply({
    summary: params.result.summary,
    artifacts: params.artifacts,
    executionVerified: params.result.executionVerified,
  });
}

function stripWorkerBoilerplate(summary: string): string {
  let s = summary.trim();
  if (!s) return "";
  s = s.replace(/^worker start[^\n]*\n+/i, "");
  s = s.replace(/^worker reply:\s*/i, "");
  return s.trim();
}

function fallbackWorkerReply(result: Extract<WorkerDispatchResult, { ok: true }>): string {
  const summary = stripWorkerBoilerplate(result.summary ?? "");
  const low = summary.toLowerCase();
  const looksLikeRoutingStatus =
    low.startsWith("routed this turn through the worker agent loop") ||
    low.startsWith("routed to worker — no conversational") ||
    low.startsWith("routed to worker (quick read/edit");
  if (looksLikeRoutingStatus) {
    return "Execution started. I’ll return a grounded answer once tool results are available.";
  }
  if (summary) return summary;
  if (!result.finalized || result.status === "running") {
    return "Running now. I will report back with tool-backed results as soon as execution completes.";
  }
  if (result.status === "error") {
    return "Execution failed. Check the run timeline for the exact error and I can retry with a narrower tool chain.";
  }
  return "Execution completed.";
}

/** Serialize writes so parallel `streamOpenRouterChat` + SSE mirror cannot interleave NDJSON lines. */
function createSerializedNdjsonWriter(write: NdjsonLineWriter): NdjsonLineWriter {
  let chain: Promise<void> = Promise.resolve();
  return async (obj) => {
    chain = chain.then(() => write(obj));
    await chain;
  };
}

function workerCompanionSystemPrompt(
  persona: string,
  kind: "skill" | "operator_execution",
  skillSlug: string | null,
): string {
  const harness = companyOsHarnessAddendum("companion");
  const pathHint =
    "**Default cwd = leased checkout root:** narrate that the worker should `ls` / `read` / `grep` from `.` and repo-relative paths (`Cargo.toml`, `src/…`, `skills/<slug>/SKILL.md`, `company-files/…`) **without** asking the operator to paste or confirm a workspace root unless telemetry shows a path error. " +
    "Do not mention lacking “shell” or “`/Users/...`” access—the worker runs in the **bound task tree** only. " +
    "This stream has no file bodies until tool events land—**never** invent file contents or shell output.";

  if (kind === "skill" && skillSlug) {
    return [
      harness,
      "",
      `You are ${persona} in the Company OS operator chat. The operator invoked skill \`${skillSlug}\`; a separate worker runtime is executing it (tools, checkout, telemetry).`,
      "Stream a concise reply in character: acknowledge the skill, describe what this class of work usually involves at a high level, and say that concrete tool traces appear as separate live events in the UI.",
      pathHint,
    ].join("\n");
  }
  return [
    harness,
    "",
    `You are ${persona} in the Company OS operator chat. The operator message is being handled by a leased worker (tools, task checkout).`,
    "Stream a concise reply: acknowledge the request, outline reasoning and next checks at a high level, and state that tool/runtime events stream separately.",
    pathHint,
  ].join("\n");
}

/** Read Company OS runtime SSE and emit each `data:` JSON payload as `{ type: "runtime", payload }`. */
export async function forwardRuntimeSseAsNdjson(
  upstreamBase: string,
  signal: AbortSignal,
  write: NdjsonLineWriter,
  onRuntimePayload?: RuntimePayloadHandler,
): Promise<void> {
  const url = `${upstreamBase.replace(/\/+$/, "")}/api/company/runtime/events/stream`;
  let res: Response;
  try {
    res = await fetch(url, {
      headers: { Accept: "text/event-stream" },
      signal,
    });
  } catch {
    return;
  }
  if (!res.ok || !res.body) return;

  const reader = res.body.getReader();
  const dec = new TextDecoder();
  let buf = "";
  for (;;) {
    let chunk: ReadableStreamReadResult<Uint8Array>;
    try {
      chunk = await reader.read();
    } catch {
      break;
    }
    if (chunk.done) break;
    buf += dec.decode(chunk.value, { stream: true });
    for (;;) {
      const sep = buf.indexOf("\n\n");
      if (sep < 0) break;
      const block = buf.slice(0, sep);
      buf = buf.slice(sep + 2);
      for (const line of block.split("\n")) {
        const t = line.trim();
        if (!t.startsWith("data:")) continue;
        const data = t.slice(5).trim();
        if (!data || data === "[DONE]") continue;
        try {
          const payload = JSON.parse(data) as Record<string, unknown>;
          if (onRuntimePayload) {
            try {
              await onRuntimePayload(payload);
            } catch {
              // Best-effort observer callback.
            }
          }
          const et = typeof payload.event_type === "string" ? payload.event_type : "";
          const inner = payload.stream_event;
          if (et === "stream_event" && inner != null && typeof inner === "object") {
            await write(wrapSdkStreamEvent(inner as Record<string, unknown>) as Record<string, unknown>);
          } else {
            if (shouldExposeRuntimePayload(payload)) {
              await write({ type: "runtime", payload });
            }
          }
        } catch {
          await write({ type: "runtime_raw", text: data });
        }
      }
    }
  }
}

async function streamAgentChat(params: {
  backend: ChatBackend;
  messages: Array<{ role: "system" | "user" | "assistant"; content: string }>;
  maxTokens: number;
  temperature: number;
  write: NdjsonLineWriter;
}): Promise<string> {
  const { backend, messages, maxTokens, temperature, write } = params;
  const res = await fetch(
    backend.provider === "openrouter" ? `${backend.baseUrl}/chat/completions` : `${backend.baseUrl}/api/chat`,
    {
      method: "POST",
      headers:
        backend.provider === "openrouter"
          ? {
              Authorization: `Bearer ${backend.apiKey}`,
              "Content-Type": "application/json",
              "HTTP-Referer": "https://hsm.ai",
              "X-Title": "HSM Company Console",
            }
          : {
              "Content-Type": "application/json",
            },
      body: JSON.stringify(
        backend.provider === "openrouter"
          ? {
              model: backend.model,
              messages,
              max_tokens: maxTokens,
              temperature,
              stream: true,
            }
          : {
              model: backend.model,
              messages,
              stream: true,
              options: {
                temperature,
                num_predict: maxTokens,
              },
            },
      ),
    },
  );

  if (!res.ok || !res.body) {
    const errText = await res.text().catch(() => res.statusText);
    throw new Error(`LLM ${res.status}: ${errText}`);
  }

  await write(wrapSdkStreamEvent(anthropicMessageStart(backend.model)) as Record<string, unknown>);
  await write(wrapSdkStreamEvent(anthropicContentBlockStartText()) as Record<string, unknown>);

  const reader = res.body.getReader();
  const dec = new TextDecoder();
  let buf = "";
  let full = "";
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    buf += dec.decode(value, { stream: true });
    const lines = buf.split("\n");
    buf = lines.pop() ?? "";
    for (const line of lines) {
      const t = line.trim();
      try {
        let piece = "";
        if (backend.provider === "openrouter") {
          if (!t.startsWith("data:")) continue;
          const data = t.slice(5).trim();
          if (data === "[DONE]") continue;
          const j = JSON.parse(data) as Record<string, unknown>;
          const choices = Array.isArray(j.choices) ? j.choices : [];
          const ch0 = asObject(choices[0]);
          piece = openRouterStreamDeltaToText(ch0?.delta);
        } else {
          const j = JSON.parse(t) as Record<string, unknown>;
          const message = asObject(j.message);
          piece = normalizeChatCompletionMessageContent(message?.content);
        }
        if (piece) {
          full += piece;
          await write(wrapSdkStreamEvent(anthropicTextDelta(piece)) as Record<string, unknown>);
        }
      } catch {
        /* skip malformed chunk */
      }
    }
  }
  const tail = buf.trim();
  if (tail) {
    if (backend.provider === "openrouter" && tail.startsWith("data:")) {
      const data = tail.slice(5).trim();
      if (data && data !== "[DONE]") {
        try {
          const j = JSON.parse(data) as Record<string, unknown>;
          const choices = Array.isArray(j.choices) ? j.choices : [];
          const ch0 = asObject(choices[0]);
          const piece = openRouterStreamDeltaToText(ch0?.delta);
          if (piece) {
            full += piece;
            await write(wrapSdkStreamEvent(anthropicTextDelta(piece)) as Record<string, unknown>);
          }
        } catch {
          /* ignore */
        }
      }
    } else if (backend.provider === "ollama") {
      try {
        const j = JSON.parse(tail) as Record<string, unknown>;
        const message = asObject(j.message);
        const piece = normalizeChatCompletionMessageContent(message?.content);
        if (piece) {
          full += piece;
          await write(wrapSdkStreamEvent(anthropicTextDelta(piece)) as Record<string, unknown>);
        }
      } catch {
        /* ignore */
      }
    }
  }
  await write(wrapSdkStreamEvent(anthropicContentBlockStop()) as Record<string, unknown>);
  await write(wrapSdkStreamEvent(anthropicMessageDelta()) as Record<string, unknown>);
  await write(wrapSdkStreamEvent(anthropicMessageStop()) as Record<string, unknown>);
  return full;
}

type SemanticTurnRoute = {
  actionable: boolean;
  confidence: number;
  reason: string;
};

function parseSemanticRouterJson(raw: string): SemanticTurnRoute | null {
  const t = raw.trim();
  if (!t) return null;
  const candidate = t.match(/\{[\s\S]*\}/)?.[0] ?? t;
  try {
    const parsed = JSON.parse(candidate) as {
      actionable?: unknown;
      confidence?: unknown;
      reason?: unknown;
    };
    if (typeof parsed.actionable !== "boolean") return null;
    const confidenceRaw = typeof parsed.confidence === "number" ? parsed.confidence : 0.5;
    const confidence = Number.isFinite(confidenceRaw)
      ? Math.max(0, Math.min(1, confidenceRaw))
      : 0.5;
    const reason = typeof parsed.reason === "string" ? parsed.reason.trim() : "";
    return {
      actionable: parsed.actionable,
      confidence,
      reason: reason || "semantic_router",
    };
  } catch {
    return null;
  }
}

async function classifySemanticTurnRoute(params: {
  backend: ChatBackend;
  notes: AgentChatRequestBody["notes"];
  lastOperatorText: string;
}): Promise<SemanticTurnRoute | null> {
  const { backend, notes, lastOperatorText } = params;
  const recent = notes
    .slice(-8)
    .map((n) => `${n.actor}: ${n.text}`.slice(0, 380))
    .join("\n");
  const prompt = [
    "Classify if the latest operator turn should be handled by the Hermes worker agent (which has full workspace/file/shell access).",
    "Use full conversational context; do not rely on keyword-only matching.",
    "actionable=true when ANY of these apply:",
    "  • Explicitly asks to read/list/edit/search/run files or shell commands",
    "  • Asks a technical question about code, architecture, or the repo that needs reading files to answer correctly (e.g. 'how does X work?', 'what does Y do?', 'explain Z')",
    "  • Requests debugging, analysis, refactoring, testing, or building anything",
    "  • Asks about errors, bugs, logs, or system behaviour",
    "actionable=false ONLY for: greetings, pure chitchat, 'how are you', weekly strategy planning unrelated to code, 'what should we focus on (business-level)', or explicit capability questions ('can you read files?').",
    "IMPORTANT: 'what does the SFT module do?' = actionable=true (needs file read). 'list 5 strategic priorities' = actionable=false (business strategy).",
    'Return JSON only: {"actionable":true|false,"confidence":0..1,"reason":"short_reason"}',
    "",
    "Conversation context:",
    recent,
    "",
    `Latest operator turn: ${lastOperatorText}`,
  ].join("\n");

  try {
    const res = await fetch(
      backend.provider === "openrouter" ? `${backend.baseUrl}/chat/completions` : `${backend.baseUrl}/api/chat`,
      {
        method: "POST",
        headers:
          backend.provider === "openrouter"
            ? {
                Authorization: `Bearer ${backend.apiKey}`,
                "Content-Type": "application/json",
                "HTTP-Referer": "https://hsm.ai",
                "X-Title": "HSM Company Console",
              }
            : {
                "Content-Type": "application/json",
              },
        body: JSON.stringify(
          backend.provider === "openrouter"
            ? {
                model: backend.model,
                messages: [
                  { role: "system", content: "You are a strict JSON classifier." },
                  { role: "user", content: prompt },
                ],
                max_tokens: 120,
                temperature: 0,
                stream: false,
              }
            : {
                model: backend.model,
                messages: [
                  { role: "system", content: "You are a strict JSON classifier." },
                  { role: "user", content: prompt },
                ],
                stream: false,
                options: {
                  temperature: 0,
                  num_predict: 120,
                },
              },
        ),
      },
    );
    if (!res.ok) return null;
    const json = (await res.json().catch(() => null)) as Record<string, unknown> | null;
    if (!json) return null;
    let out = "";
    if (backend.provider === "openrouter") {
      const choices = Array.isArray(json.choices) ? json.choices : [];
      const ch0 = asObject(choices[0]);
      const message = asObject(ch0?.message);
      out = normalizeChatCompletionMessageContent(message?.content).trim();
    } else {
      const message = asObject(json.message);
      out = normalizeChatCompletionMessageContent(message?.content).trim();
    }
    return parseSemanticRouterJson(out);
  } catch {
    return null;
  }
}

async function withRuntimeMirror(
  upstream: string,
  write: NdjsonLineWriter,
  work: () => Promise<void>,
  onRuntimePayload?: RuntimePayloadHandler,
): Promise<void> {
  const ac = new AbortController();
  const mirror = forwardRuntimeSseAsNdjson(upstream, ac.signal, write, onRuntimePayload);
  try {
    await work();
  } finally {
    ac.abort();
  }
  await mirror.catch(() => {});
}

/**
 * Full operator-chat pipeline writing NDJSON lines (same branching as POST /api/agent-chat-reply).
 */
export async function runAgentChatNdjsonStream(body: AgentChatRequestBody, write: NdjsonLineWriter): Promise<void> {
  const { taskId, persona, companyId, notes } = body;
  const rawLastOperatorText = [...notes].reverse().find((n) => n.actor === "operator")?.text ?? "";
  let lastOperatorText = rawLastOperatorText;
  const policyCheck = evaluateStrictActionPolicy(rawLastOperatorText);
  if (policyCheck.blocked) {
    await write({ type: "error", message: policyCheck.reason ?? "Blocked by strict security policy" });
    return;
  }

  const sentinelCommand = parseSentinelReputationCommand(rawLastOperatorText);
  if (sentinelCommand) {
    await write({ type: "phase", phase: "sentinel_reputation", query: sentinelCommand.query });
    const sentinel = await querySentinelReputation({ query: sentinelCommand.query });
    if (!sentinel.ok) {
      await write({
        type: "error",
        message: sentinel.error ?? "Sentinel reputation query failed",
        sentinel,
      });
      return;
    }
    await write({
      type: "done",
      ok: true,
      reply: `Sentinel reputation for "${sentinelCommand.query}" retrieved.`,
      at: new Date().toISOString(),
      sentinel,
      execution_mode: "worker",
      worker_evidence: true,
      execution_verified: true,
      finalized: true,
    });
    return;
  }

  let agentRegistryId: string | undefined;
  let agentAdapterConfig: Record<string, unknown> | null = null;
  let detectedSkill: string | null = null;
  let slashCommand: ReturnType<typeof parseAgentChatSlashCommand> = null;

  if (companyId) {
    const { agentRegistryId: aid, allKnownSlugs, agentAdapterConfig: cfg } = await resolveAgentForPersona(
      companyId,
      persona,
    );
    agentRegistryId = aid;
    agentAdapterConfig = cfg;
    slashCommand = parseAgentChatSlashCommand(rawLastOperatorText, allKnownSlugs);
    if (slashCommand) {
      lastOperatorText = slashCommandWorkerInstruction(slashCommand);
      if (slashCommand.kind === "skill" && slashCommand.skillSlug) {
        detectedSkill = slashCommand.skillSlug;
      }
      await write({
        type: "phase",
        phase: "slash_command",
        command: slashCommand.kind,
        skill: slashCommand.kind === "skill" ? slashCommand.skillSlug : undefined,
      });
    } else {
      detectedSkill = detectSkillDispatch(notes, allKnownSlugs);
    }
  }

  const strictFlow = companyId ? await buildStrictToolFlowTrace(companyId, lastOperatorText) : null;

  if (detectedSkill && companyId) {
    await write({ type: "phase", phase: "skill_dispatch", skill: detectedSkill });
    const policy = deriveToolExecutionPolicy(agentAdapterConfig);
    const compact = await buildCompactedContextBundle({
      companyId,
      taskId,
      agentRegistryId,
      budgetBytes: 5200,
    });
    const companionOptOut = process.env.HSM_DISABLE_WORKER_COMPANION_STREAM === "1";
    const workerCompanionEnabled = process.env.HSM_WORKER_COMPANION_STREAM === "1";
    const companionBackend = companionOptOut || !workerCompanionEnabled ? null : readAgentChatBackend();
    await write({
      type: "phase",
      phase: "skill_dispatch_runtime",
      live_companion_stream: !!companionBackend,
    });
    const ser = createSerializedNdjsonWriter(write);
    const runArtifacts = new Map<string, MutableRunArtifact>();
    const operationalState = makeOperationalAccumulator();
    let result: WorkerDispatchResult | undefined;
    await withRuntimeMirror(
      UPSTREAM,
      ser,
      async () => {
      const dispatchPromise = dispatchSkillToWorker({
        companyId,
        taskId,
        persona,
        skillSlug: detectedSkill,
        externalSystem: "worker-dispatch",
        persistAgentNote: true,
        waitForTelemetryMs: BUILD_HEAVY_SKILLS.has(detectedSkill)
          ? Math.max(240_000, OPERATOR_CHAT_TELEMETRY_WAIT_EXEC_MS)
          : OPERATOR_CHAT_TELEMETRY_WAIT_EXEC_MS,
        requireWorkerEvidence: true,
        runSummary: `Skill turn via agent loop runtime: ${detectedSkill}`,
        extraMeta: {
          trigger: "operator_chat",
          operator_message: lastOperatorText.slice(0, 600),
          loop_state: "running",
          strict_tool_flow: strictFlow,
          tool_execution_policy: policy,
          compact_context: {
            bytes: compact.bytes,
            sections: compact.sections,
            text: compact.compactText,
          },
        },
      });
      if (companionBackend) {
        const companion = streamAgentChat({
          backend: companionBackend,
          messages: [
            { role: "system", content: workerCompanionSystemPrompt(persona, "skill", detectedSkill) },
            { role: "user", content: lastOperatorText.slice(0, 8000) },
          ],
          maxTokens: 640,
          temperature: 0.45,
          write: ser,
        }).catch(() => "");
        const [, r] = await Promise.all([companion, dispatchPromise]);
        result = r;
      } else {
        result = await dispatchPromise;
      }
      },
      (payload) => {
        captureRunArtifactFromRuntime(payload, runArtifacts);
        captureOperationalStateFromRuntime(payload, operationalState);
      },
    );
    const artifacts = serializeRunArtifacts(runArtifacts);
    await patchRunArtifactsMeta(companyId, result?.runId, artifacts);
    if (!result || result.ok !== true) {
      const r = result;
      await write({
        type: "error",
        message: r && r.ok === false ? r.error : "dispatch failed",
        run_id: r ? r.runId ?? undefined : undefined,
        http_status: r && r.ok === false ? r.httpStatus : undefined,
      });
      return;
    }
    await upsertThreadSessionState({
      companyId,
      persona,
      taskId,
      runId: result.runId ?? undefined,
      state: {
        mode: "skill",
        skill: detectedSkill,
        loop_state: result.status === "running" ? "running" : "completed",
        compact_bytes: compact.bytes,
      },
    });
    const finalizedSummary =
      typeof result.summary === "string" && result.summary.trim().length > 0 ? result.summary.trim() : "";
    const skillReply = result.finalized
      ? finalizedSummary || `Completed \`${detectedSkill}\` in worker agent runtime.`
      : `Dispatched \`${detectedSkill}\` to worker runtime. Waiting for completion evidence.`;
    await write({
      type: "done",
      ok: true,
      reply: skillReply,
      at: new Date().toISOString(),
      run_id: result.runId,
      skill: detectedSkill,
      status: result.status,
      execution_mode: result.executionMode,
      worker_evidence: result.workerEvidence,
      execution_verified: result.executionVerified,
      finalized: result.finalized,
    });
    if (result.finalized) {
      await write({
        type: "final_answer",
        payload: {
          message: skillReply,
        },
      });
    }
    return;
  }

  if (!detectedSkill && companyId) {
    // ── Task Management Action Lane ──────────────────────────────────────────
    // Handles "create task", "assign to", "hand back", "mark done", "add note",
    // "requires human" deterministically without going through worker or LLM.
    const taskAction = parseTaskManagementAction(lastOperatorText);
    if (taskAction) {
      await write({ type: "phase", phase: "task_action", action_kind: taskAction.kind });
      const actionResult = await executeTaskAction({
        action: taskAction,
        companyId,
        taskId,
        persona,
      });
      await write({
        type: "done",
        ok: actionResult.ok,
        reply: actionResult.message,
        at: new Date().toISOString(),
        task_action: actionResult.kind,
        task_id: actionResult.taskId,
        data: actionResult.data,
      });
      return;
    }

    const optimize = parseOptimizeCommand(lastOperatorText);
    if (optimize) {
      await write({ type: "phase", phase: "optimize", optimize_kind: optimize.kind });
      const compact = await buildCompactedContextBundle({
        companyId,
        taskId,
        agentRegistryId,
        budgetBytes: 5200,
      });
      const policy = deriveToolExecutionPolicy(agentAdapterConfig);
      const optimizeRunId = await createAgentRun(companyId, agentRegistryId, taskId, "optimize_anything", {
        externalSystem: "optimize-chat",
        executionMode: "pending",
        summary: `Optimize command: ${lastOperatorText.slice(0, 140)}`,
      });
      if (!optimizeRunId) {
        await write({ type: "error", message: "Failed to create optimize run" });
        return;
      }

      let endpoint = `${UPSTREAM}/api/optimize`;
      let optimizeBody: Record<string, unknown> = {
        artifact: {
          kind: "company_task",
          company_id: companyId,
          task_id: taskId,
          persona,
          request: lastOperatorText,
        },
      };
      if (optimize.kind === "plan") {
        endpoint = `${UPSTREAM}/api/plan/optimize`;
        optimizeBody = { step_index: optimize.stepIndex };
      } else if (optimize.kind === "signature") {
        endpoint = `${UPSTREAM}/api/dspy/optimize`;
        optimizeBody = { signature_name: optimize.signatureName };
      }

      const optRes = await fetch(endpoint, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(optimizeBody),
      });
      const optRaw = await optRes.json().catch(() => ({}));
      const optObj = asObject(optRaw);
      const optJson = {
        error: typeof optObj?.error === "string" ? optObj.error : undefined,
        result: typeof optObj?.result === "string" ? optObj.result : undefined,
      };
      if (!optRes.ok) {
        await finalizeAgentRun(companyId, optimizeRunId, `Optimize failed: ${optJson.error ?? optRes.statusText}`, "error");
        await write({ type: "error", message: optJson.error ?? "optimize failed" });
        return;
      }

      const optimizeReply =
        optimize.kind === "plan"
          ? `Optimization started for plan step ${optimize.stepIndex}.`
          : optimize.kind === "signature"
            ? `DSPy optimization started for signature "${optimize.signatureName}".`
            : "OptimizeAnything session started for this task.";
      await fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${optimizeRunId}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          meta: {
            skill: "optimize_anything",
            triggered_by: "optimize-chat",
            execution_mode: "worker",
            loop_state: "completed",
            strict_tool_flow: strictFlow,
            operator_message: lastOperatorText.slice(0, 1000),
            tool_execution_policy: policy,
            compact_context: {
              bytes: compact.bytes,
              sections: compact.sections,
              text: compact.compactText,
            },
          },
        }),
      }).catch(() => {});
      await finalizeAgentRun(companyId, optimizeRunId, optimizeReply, "success");
      await upsertThreadSessionState({
        companyId,
        persona,
        taskId,
        runId: optimizeRunId,
        state: {
          mode: `optimize:${optimize.kind}`,
          loop_state: "completed",
          compact_bytes: compact.bytes,
        },
      });
      const noteRes = await fetch(`${UPSTREAM}/api/company/tasks/${taskId}/stigmergic-note`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ text: optimizeReply, actor: persona }),
      });
      const noteRaw = await noteRes.json().catch(() => ({}));
      const noteObj = asObject(noteRaw);
      await write({
        type: "done",
        ok: true,
        reply: optimizeReply,
        at: new Date().toISOString(),
        run_id: optimizeRunId,
        skill: `optimize:${optimize.kind}`,
        context_notes: noteObj?.context_notes,
      });
      return;
    }
  }

  if (!detectedSkill && companyId) {
    const chatBackend = readAgentChatBackend();
    const baseRoute = operatorChatShouldRouteWorker({
      lastOperatorText,
      hasChatBackend: !!chatBackend,
    });
    const semanticRouterEnabled = process.env.HSM_OPERATOR_CHAT_SEMANTIC_ROUTER !== "0";
    const workerFirstMode =
      process.env.HSM_OPERATOR_CHAT_WORKER_FIRST === "1" ||
      process.env.HSM_OPERATOR_CLAUDE_CODE_MODE === "1";
    const semanticRoute =
      semanticRouterEnabled && chatBackend && workerFirstMode
        ? await classifySemanticTurnRoute({
            backend: chatBackend,
            notes,
            lastOperatorText,
          })
        : null;
    // Hard file/workspace signals — these override a conversational semantic result because
    // the semantic router cannot see the workspace context and may under-classify file queries.
    const hardFileActionableIntent =
      looksLikeRepoInfoQuestion(lastOperatorText) ||
      looksLikeImplicitWorkspacePointer(lastOperatorText) ||
      looksLikeQuickToolIntent(lastOperatorText);
    const routeDecision =
      semanticRoute && baseRoute.reason === "worker_first_claude_code_mode"
        ? {
            ...baseRoute,
            // Semantic router is authoritative for its domain:
            //   • actionable=true  → worker (file/tool work confirmed by LLM)
            //   • actionable=false → companion LLM answers from combined_system_addon context
            //       EXCEPT when hard heuristics have high confidence it's a file query
            //       (repo info / workspace pointer / quick-tool read-edit path).
            routeWorker: semanticRoute.actionable
              ? true
              : hardFileActionableIntent && semanticRoute.confidence < 0.75,
            executionIntent:
              (semanticRoute.actionable || (hardFileActionableIntent && semanticRoute.confidence < 0.75))
                ? true
                : baseRoute.executionIntent,
            reason: semanticRoute.actionable
              ? "semantic_actionable"
              : hardFileActionableIntent && semanticRoute.confidence < 0.75
                ? "heuristic_file_low_confidence_override"
                : "semantic_conversational",
          }
        : baseRoute;
    const { routeWorker, executionIntent, codingIntent, quickToolIntent, reason: workerRouteReason } = routeDecision;
    await write({
      type: "phase",
      phase: "turn_router",
      route_worker: routeWorker,
      route_reason: workerRouteReason,
      router: semanticRoute ? "semantic+heuristic" : "heuristic",
      semantic_confidence: semanticRoute?.confidence,
      semantic_reason: semanticRoute?.reason,
    });
    if (routeWorker) {
      const companionOptOut = process.env.HSM_DISABLE_WORKER_COMPANION_STREAM === "1";
      const workerCompanionEnabled = process.env.HSM_WORKER_COMPANION_STREAM === "1";
      // Companion is opt-in; when enabled you can opt out of coding/execution narration with `...WITH_CODING=0`.
      const companionWithCoding = process.env.HSM_WORKER_COMPANION_WITH_CODING !== "0";
      const skillsCatalogQ = looksLikeSkillsOrCatalogQuestion(lastOperatorText);
      const skipCompanionForCodingTurn =
        !companionWithCoding && (executionIntent || codingIntent) && !skillsCatalogQ;
      const companionBackend =
        companionOptOut || !workerCompanionEnabled || skipCompanionForCodingTurn ? null : chatBackend;
      await write({
        type: "phase",
        phase: "operator_chat_worker",
        execution_intent: executionIntent,
        coding_intent: codingIntent,
        quick_tool_intent: quickToolIntent,
        chat_backend: chatBackend?.label ?? null,
        token_stream: !!companionBackend,
        companion_sidecar: !!companionBackend,
        companion_skipped_for_coding: skipCompanionForCodingTurn,
        route_reason: workerRouteReason,
      });
      const policy = deriveToolExecutionPolicy(agentAdapterConfig);
      const compact = await buildCompactedContextBundle({
        companyId,
        taskId,
        agentRegistryId,
        budgetBytes: 5200,
      });
      const ser = createSerializedNdjsonWriter(write);
      const runArtifacts = new Map<string, MutableRunArtifact>();
      const operationalState = makeOperationalAccumulator();
      let result: WorkerDispatchResult | undefined;
      await withRuntimeMirror(
        UPSTREAM,
        ser,
        async () => {
        const quickToolMode = operatorChatQuickToolPromptMode({
          quickToolIntent,
          routeReason: workerRouteReason,
        });
        const dispatchPromise = dispatchSkillToWorker({
          companyId,
          taskId,
          persona,
          skillSlug: "operator-chat",
          externalSystem: "worker-dispatch-chat",
          persistAgentNote: true,
          waitForTelemetryMs:
            executionIntent || codingIntent || quickToolIntent
              ? OPERATOR_CHAT_TELEMETRY_WAIT_EXEC_MS
              : OPERATOR_CHAT_TELEMETRY_WAIT_ANALYSIS_MS,
          requireWorkerEvidence: true,
          runSummary: `Operator turn via agent loop runtime: ${lastOperatorText.slice(0, 120)}`,
          extraMeta: {
            trigger: "operator_chat",
            operator_message: lastOperatorText.slice(0, 1000),
            intent: executionIntent || codingIntent ? "execution" : "analysis",
            loop_state: "running",
            strict_tool_flow: strictFlow,
            tool_execution_policy: policy,
            quick_tool_mode: quickToolMode,
            compact_context: {
              bytes: compact.bytes,
              sections: compact.sections,
              text: compact.compactText,
            },
          },
        });
        if (companionBackend) {
          const companion = streamAgentChat({
            backend: companionBackend,
            messages: [
              {
                role: "system",
                content: workerCompanionSystemPrompt(persona, "operator_execution", null),
              },
              { role: "user", content: lastOperatorText.slice(0, 8000) },
            ],
            maxTokens: 768,
            temperature: 0.45,
            write: ser,
          }).catch(() => "");
          const [, r] = await Promise.all([companion, dispatchPromise]);
          result = r;
        } else {
          result = await dispatchPromise;
        }
        },
        (payload) => {
          captureRunArtifactFromRuntime(payload, runArtifacts);
          captureOperationalStateFromRuntime(payload, operationalState);
        },
      );
      const artifacts = serializeRunArtifacts(runArtifacts);
      await patchRunArtifactsMeta(companyId, result?.runId, artifacts);
      if (!result || result.ok !== true) {
        const r = result;
        await write({
          type: "error",
          message: r && r.ok === false ? r.error : "dispatch failed",
          run_id: r ? r.runId ?? undefined : undefined,
          http_status: r && r.ok === false ? r.httpStatus : undefined,
        });
        return;
      }
      await upsertThreadSessionState({
        companyId,
        persona,
        taskId,
        runId: result.runId ?? undefined,
        state: {
          mode: "operator_chat_turn",
          loop_state: result.status === "running" ? "running" : "completed",
          compact_bytes: compact.bytes,
        },
      });
      const executionReply = maybeExecutionReply({
        executionIntent,
        codingIntent,
        result,
        artifacts,
      });
      const finalReply = executionReply ?? fallbackWorkerReply(result);
      await write({
        type: "done",
        ok: true,
        reply: finalReply,
        at: new Date().toISOString(),
        run_id: result.runId,
        skill: "worker-agent-loop",
        status: result.status,
        execution_mode: result.executionMode,
        worker_evidence: result.workerEvidence,
        execution_verified: result.executionVerified,
        finalized: result.finalized,
        streaming: true,
      });
      return;
    }
    if (!chatBackend) {
      await write({
        type: "error",
        message:
          "No agent chat backend configured for Next.js. Set HSM_AGENT_CHAT_PROVIDER=ollama with OLLAMA_URL/OLLAMA_MODEL for local chat, or OPENROUTER_API_KEY for OpenRouter.",
      });
      return;
    }
    await write({
      type: "phase",
      phase: `${chatBackend.label}_llm`,
      chat_mode: "conversational_company",
      chat_backend: chatBackend.label,
    });
  }

  const chatBackend = readAgentChatBackend();
  if (!chatBackend) {
    await write({
      type: "error",
      message:
        "No agent chat backend configured for Next.js. Set HSM_AGENT_CHAT_PROVIDER=ollama with OLLAMA_URL/OLLAMA_MODEL for local chat, or OPENROUTER_API_KEY for OpenRouter.",
    });
    return;
  }

  await write({ type: "phase", phase: `${chatBackend.label}_llm`, chat_backend: chatBackend.label });
  const thin = isThinHarnessModel(chatBackend.model);
  if (thin) {
    await write({ type: "phase", phase: "thin_harness", model: chatBackend.model });
  }
  const system = await Promise.race([
    buildSystemPrompt(persona, companyId, detectedSkill, taskId, "operator_chat", thin),
    new Promise<string>((resolve) =>
      setTimeout(() => resolve(`You are ${persona}, an AI agent. Be concise and in-character.`), 7000),
    ),
  ]);
  let enrichedSystem = system;
  if (companyId) {
    const [taskCtxData, threadData] = await Promise.all([
      fetch(`${UPSTREAM}/api/company/tasks/${taskId}/llm-context`).then((r) => r.json()).catch(() => null),
      agentRegistryId
        ? fetch(`${UPSTREAM}/api/company/companies/${companyId}/agents/${agentRegistryId}/operator-thread`)
            .then((r) => r.json())
            .catch(() => null)
        : Promise.resolve(null),
    ]);
    const addon = (taskCtxData as { combined_system_addon?: string } | null)?.combined_system_addon ?? "";
    const compactDigest = (threadData as { compact_digest?: string } | null)?.compact_digest ?? "";
    // Thin mode: dramatically tighten enrichment caps so the model has headroom
    const addonCap   = thin ? 600  : 2800;
    const digestCap  = thin ? 400  : 1800;
    const compact = [
      compactDigest ? `## Operator Thread Digest\n${compactDigest.slice(0, digestCap)}` : "",
      addon ? `## Task LLM Context (Compacted)\n${addon.slice(0, addonCap)}` : "",
    ]
      .filter(Boolean)
      .join("\n\n");
    if (compact) {
      enrichedSystem = `${system}\n\n${compact}`;
    }
  }

  // Compact the note history if it has grown large — keeps LLM cost/latency
  // bounded and avoids context-window overflows on long operator threads.
  const compaction = compactNotesForLlm(notes);
  if (compaction.compacted) {
    await write({
      type: "phase",
      phase: "context_compaction",
      notes_compacted: compaction.compactedCount,
      // Verbatim turns sent to the LLM (matches `compactNotesForLlm` slice, not raw `notes.length`).
      notes_kept: compaction.messageHistory.length,
    });
    // Fold the prose summary into the system prompt so the model has the full
    // thread narrative without it counting against the message-turn budget.
    // Thin mode: cap to 600 chars so a small free model isn't overwhelmed.
    const summaryToInject = thin
      ? (compaction.compactionSummary ?? "").slice(0, 600)
      : (compaction.compactionSummary ?? "");
    enrichedSystem = `${enrichedSystem}\n\n${summaryToInject}`;
    // Persist to supermemory so the history is searchable later.
    if (companyId && compaction.compactionSummary) {
      saveCompactionToMemory(companyId, taskId, persona, compaction.compactionSummary).catch(() => {});
    }
  }

  const messages = compaction.messageHistory;

  if (messages.length === 0 || messages[messages.length - 1].role !== "user") {
    await write({ type: "error", message: "No operator message to respond to" });
    return;
  }

  let reply: string;
  try {
    reply = await streamAgentChat({
      backend: chatBackend,
      messages: [{ role: "system", content: enrichedSystem }, ...messages],
      maxTokens: detectedSkill ? 1024 : 768,
      temperature: detectedSkill ? 0.4 : 0.7,
      write,
    });
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    await write({ type: "error", message: msg });
    return;
  }

  reply = reply.trim();
  if (!reply) {
    await write({ type: "error", message: "Empty response from LLM" });
    return;
  }

  const noteRes = await fetch(`${UPSTREAM}/api/company/tasks/${taskId}/stigmergic-note`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ text: reply, actor: persona }),
  });
  const noteRaw = await noteRes.json().catch(() => ({}));
  const noteObj = asObject(noteRaw);

  await write({
    type: "done",
    ok: true,
    reply,
    at: new Date().toISOString(),
    skill: detectedSkill ?? undefined,
    context_notes: noteObj?.context_notes,
  });
}
