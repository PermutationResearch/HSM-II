/**
 * Shared server-only helpers for operator chat + skill execution (OpenRouter + Company OS).
 */

import { canTransitionRunLoopState, type RunLoopState } from "@/app/lib/runtime-contract";

export const UPSTREAM = (process.env.HSM_CONSOLE_URL ?? "http://127.0.0.1:3847").replace(/\/+$/, "");
export const OR_BASE = (process.env.OPENROUTER_API_BASE ?? "https://openrouter.ai/api/v1").replace(/\/+$/, "");

/** OpenRouter key for server-side chat (Next App Routes). Checks alternate env name used in some deployments. */
export function readOpenRouterApiKey(): string | undefined {
  const a = process.env.OPENROUTER_API_KEY;
  const b = process.env.HSM_OPENROUTER_API_KEY;
  const s = typeof a === "string" && a.trim() ? a.trim() : typeof b === "string" && b.trim() ? b.trim() : "";
  return s || undefined;
}

export function normalizeModel(m: string): string {
  return m.replace(/^openrouter\//, "");
}

const DEFAULT_MODEL = "openai/gpt-oss-120b:free";

export const CHAT_MODEL = normalizeModel(
  process.env.HSM_AGENT_CHAT_MODEL ?? process.env.DEFAULT_LLM_MODEL ?? DEFAULT_MODEL,
);

export type StigNote = { at: string; actor: string; text: string };

export type OptimizeCommand =
  | { kind: "plan"; stepIndex: number }
  | { kind: "signature"; signatureName: string }
  | { kind: "task" };

export function looksLikeExecutionIntent(text: string): boolean {
  const t = text.trim().toLowerCase();
  if (!t) return false;
  return /^(please\s+)?(run|do|execute|fix|implement|build|search|grep|read|edit|write|analyze)\b/.test(t);
}

export function parseOptimizeCommand(text: string): OptimizeCommand | null {
  const t = text.trim();
  if (!/^optimize\b/i.test(t)) return null;
  const m = /^optimize\s*(.*)$/i.exec(t);
  const rest = (m?.[1] ?? "").trim();
  if (!rest || /^task\b/i.test(rest)) return { kind: "task" };
  const planMatch = /^plan(?:\s+(\d+))?/i.exec(rest);
  if (planMatch) {
    const stepIndex = Number.parseInt(planMatch[1] ?? "0", 10);
    return { kind: "plan", stepIndex: Number.isFinite(stepIndex) ? Math.max(0, stepIndex) : 0 };
  }
  const sigMatch = /^signature\s+(.+)$/i.exec(rest);
  if (sigMatch && sigMatch[1].trim()) {
    return { kind: "signature", signatureName: sigMatch[1].trim() };
  }
  return { kind: "task" };
}

export interface AgentRecord {
  id?: string;
  agent_ref?: string;
  title?: string;
  role?: string;
  briefing?: string;
  adapter_config?: {
    paperclip?: { skills?: string[]; agent_dir?: string };
  };
}

export interface SkillRecord {
  slug: string;
  description?: string;
}

interface MemoryRecord {
  title?: string;
  content?: string;
  kind?: string;
}

export type ToolExecutionPolicySnapshot = {
  sandbox_mode: "observe" | "workspace_write" | "capability_wasm";
  allowed_tools: string[];
  network_boundary: { allowed_hosts: string[]; block_network_for_bash: boolean };
  exfiltration: { enabled: boolean; max_output_chars: number };
};

export type CompactedContextBundle = {
  compactText: string;
  bytes: number;
  sections: Array<{ name: string; bytes: number; tier: 0 | 1 | 2 }>;
};


/** Fetch with a timeout; returns null on any error. */
export async function safeFetch(url: string, timeoutMs = 3000): Promise<unknown | null> {
  try {
    const ctrl = new AbortController();
    const timer = setTimeout(() => ctrl.abort(), timeoutMs);
    const res = await fetch(url, { signal: ctrl.signal });
    clearTimeout(timer);
    if (!res.ok) return null;
    return await res.json();
  } catch {
    return null;
  }
}

export async function patchRunLoopState(params: {
  companyId: string;
  runId: string;
  currentMeta?: Record<string, unknown> | null;
  from: RunLoopState;
  to: RunLoopState;
  extraMeta?: Record<string, unknown>;
}): Promise<boolean> {
  const { companyId, runId, currentMeta, from, to, extraMeta } = params;
  if (!canTransitionRunLoopState(from, to)) return false;
  try {
    await fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${runId}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        meta: { ...(currentMeta ?? {}), loop_state: to, ...(extraMeta ?? {}) },
      }),
    });
    return true;
  } catch {
    return false;
  }
}

export type StrictToolFlowTrace = {
  query: string;
  discovered_tool_keys: string[];
  described_tool_key: string | null;
  dry_run_execution_id: string | null;
};

/**
 * Enforce discover -> describe -> (dry-run) call against company catalog.
 * This is used to stamp strict tool-flow provenance onto chat and skill runs.
 */
export async function buildStrictToolFlowTrace(
  companyId: string,
  query: string,
): Promise<StrictToolFlowTrace | null> {
  const q = query.trim();
  if (!q) return null;
  try {
    const discoverRes = await fetch(`${UPSTREAM}/api/company/companies/${companyId}/tools/discover`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ query: q, limit: 5 }),
    });
    if (!discoverRes.ok) return null;
    const discoverJson = (await discoverRes.json()) as {
      matches?: Array<{ tool_key?: string }>;
    };
    const discovered = (discoverJson.matches ?? [])
      .map((m) => (m.tool_key ?? "").trim())
      .filter(Boolean);
    if (discovered.length === 0) {
      return { query: q, discovered_tool_keys: [], described_tool_key: null, dry_run_execution_id: null };
    }
    const describedTool = discovered[0];
    const describeRes = await fetch(
      `${UPSTREAM}/api/company/companies/${companyId}/tools/${encodeURIComponent(describedTool)}/describe`,
    );
    if (!describeRes.ok) return { query: q, discovered_tool_keys: discovered, described_tool_key: null, dry_run_execution_id: null };

    const callRes = await fetch(
      `${UPSTREAM}/api/company/companies/${companyId}/tools/${encodeURIComponent(describedTool)}/call`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          dry_run: true,
          args: {},
          flow: {
            discovered_tool_keys: discovered,
            described_tool_key: describedTool,
          },
        }),
      },
    );
    const callJson = (await callRes.json().catch(() => ({}))) as { execution?: { id?: string } };
    return {
      query: q,
      discovered_tool_keys: discovered,
      described_tool_key: describedTool,
      dry_run_execution_id: callRes.ok ? (callJson.execution?.id ?? null) : null,
    };
  } catch {
    return null;
  }
}

/** Fetch raw text file from workspace; returns null if missing or too slow. */
export async function fetchWorkspaceFile(companyId: string, path: string): Promise<string | null> {
  const data = await safeFetch(
    `${UPSTREAM}/api/company/companies/${companyId}/workspace/file?path=${encodeURIComponent(path)}`,
    4000,
  );
  if (!data || typeof data !== "object") return null;
  const content = (data as Record<string, unknown>).content;
  return typeof content === "string" ? content : null;
}

/** Map free-text / bracket hint to a canonical slug from the allow-list. */
export function resolveSkillSlugHint(hint: string, slugs: string[]): string | null {
  const h = hint.trim().toLowerCase().replace(/\s+/g, " ");
  if (!h) return null;
  for (const slug of slugs) {
    if (slug.toLowerCase() === h) return slug;
  }
  const hDash = h.replace(/\s+/g, "-");
  for (const slug of slugs) {
    if (slug.toLowerCase() === hDash) return slug;
  }
  for (const slug of slugs) {
    const sl = slug.toLowerCase();
    const base = sl.split("/").pop() ?? sl;
    if (base === hDash || base === h.replace(/-/g, " ")) return slug;
  }
  return null;
}

/**
 * Detect if the last operator message is a skill dispatch command.
 * Supports `run [skill-slug]`, `run skill-slug`, /run, execute, etc.
 */
export function detectSkillDispatch(notes: StigNote[], mySkillSlugs: string[]): string | null {
  const lastOp = [...notes].reverse().find((n) => n.actor === "operator");
  if (!lastOp) return null;
  return detectSkillDispatchFromText(lastOp.text, mySkillSlugs);
}

export function detectSkillDispatchFromText(text: string, mySkillSlugs: string[]): string | null {
  const raw = text.trim();
  if (!raw) return null;
  const lower = raw.toLowerCase();

  const bracket = /\brun\s+\[([^\]]+)\]/i.exec(raw);
  if (bracket) {
    const resolved = resolveSkillSlugHint(bracket[1], mySkillSlugs);
    if (resolved) return resolved;
  }

  const runToken = /^\s*run\s+([^\s\[\]]+)/i.exec(raw);
  if (runToken) {
    const resolved = resolveSkillSlugHint(runToken[1], mySkillSlugs);
    if (resolved) return resolved;
  }

  for (const slug of mySkillSlugs) {
    const s = slug.toLowerCase();
    const patterns = [
      `run ${s}`,
      `/run ${s}`,
      `execute ${s}`,
      `trigger ${s}`,
      `run the ${s}`,
      `start ${s}`,
    ];
    if (patterns.some((p) => lower.includes(p) || lower === s)) return slug;
  }
  return null;
}

export type CreateAgentRunOptions = {
  externalSystem?: string;
  externalRunId?: string;
  summary?: string;
  executionMode?: "worker" | "llm_simulated" | "pending";
};

/** POST to agent-runs and return the run id, or null on failure. */
export async function createAgentRun(
  companyId: string,
  agentId: string | undefined,
  taskId: string,
  skillSlug: string,
  opts?: CreateAgentRunOptions,
): Promise<string | null> {
  try {
    const external_system = (opts?.externalSystem ?? "operator-chat").trim() || "operator-chat";
    const execution_mode =
      opts?.executionMode ?? (external_system === "operator-chat" ? "llm_simulated" : "worker");
    const computedExternalRunId =
      opts?.externalRunId?.trim() ||
      (external_system === "operator-chat"
        ? undefined
        : `${external_system}:${taskId}:${Date.now()}`);
    const body: Record<string, unknown> = {
      task_id: taskId,
      company_agent_id: agentId ?? null,
      external_system,
      summary: opts?.summary ?? `Skill dispatched: ${skillSlug}`,
      meta: { skill: skillSlug, triggered_by: external_system, execution_mode },
    };
    if (computedExternalRunId) {
      body.external_run_id = computedExternalRunId;
    }
    const res = await fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!res.ok) return null;
    const j = (await res.json()) as { run?: { id?: string } };
    return j.run?.id ?? null;
  } catch {
    return null;
  }
}

/** PATCH agent-run with final status + summary. */
export async function finalizeAgentRun(
  companyId: string,
  runId: string,
  summary: string,
  status: "success" | "error",
): Promise<void> {
  try {
    await fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${runId}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ status, summary, finished_at: true }),
    });
  } catch {
    /* best-effort */
  }
}

export type PromptAudience = "operator_chat" | "headless";

export async function buildSystemPrompt(
  persona: string,
  companyId: string | undefined,
  skillSlug: string | null,
  taskId: string,
  audience: PromptAudience = "operator_chat",
): Promise<string> {
  const now = new Date().toLocaleString("en-US", {
    weekday: "long",
    year: "numeric",
    month: "long",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    timeZoneName: "short",
  });

  if (!companyId) {
    return [
      `You are ${persona}, an AI agent. Today is ${now}.`,
      audience === "operator_chat"
        ? `You are in a direct operator chat. Be concise and in-character.`
        : `Execute the requested skill and report results clearly.`,
    ].join("\n");
  }

  const fetchTaskCtx = skillSlug
    ? safeFetch(`${UPSTREAM}/api/company/tasks/${taskId}/llm-context`, 5000)
    : Promise.resolve(null);

  const [agentsData, skillsData, memoryData, visionContent, taskCtx] = await Promise.all([
    safeFetch(`${UPSTREAM}/api/company/companies/${companyId}/agents`),
    safeFetch(`${UPSTREAM}/api/company/companies/${companyId}/skills`),
    safeFetch(`${UPSTREAM}/api/company/companies/${companyId}/memory`),
    fetchWorkspaceFile(companyId, "VISION.md"),
    fetchTaskCtx,
  ]);

  const agents: AgentRecord[] = (agentsData as { agents?: AgentRecord[] })?.agents ?? [];
  const me =
    agents.find(
      (a) =>
        a.agent_ref === persona ||
        a.title?.toLowerCase() === persona.toLowerCase() ||
        (a.adapter_config?.paperclip?.agent_dir ?? "").includes(persona),
    ) ?? agents.find((a) => a.title?.toLowerCase().includes(persona.toLowerCase()));

  const skills: SkillRecord[] = (skillsData as { skills?: SkillRecord[] })?.skills ?? [];
  const memories: MemoryRecord[] = (memoryData as { memories?: MemoryRecord[] })?.memories ?? [];

  const mySkillSlugs = me?.adapter_config?.paperclip?.skills ?? [];
  const mySkills =
    mySkillSlugs.length > 0 ? skills.filter((s) => mySkillSlugs.includes(s.slug)) : [];

  const teammates = agents
    .filter((a) => a !== me && a.title)
    .map((a) => `- **${a.title}** (${a.role ?? a.agent_ref ?? "agent"})`);

  const parts: string[] = [];

  if (me?.briefing) {
    parts.push(me.briefing.trim());
  } else {
    const label = me?.title ?? persona;
    const roleStr = me?.role ? ` — ${me.role}` : "";
    parts.push(`You are ${label}${roleStr} at this company.`);
  }

  parts.push(`\nToday is ${now}.`);

  if (visionContent) {
    const snippet =
      visionContent.length > 2000 ? visionContent.slice(0, 2000) + "\n…[truncated]" : visionContent;
    parts.push(`\n## Company Vision (VISION.md)\n${snippet}`);
  }

  if (audience === "operator_chat") {
    parts.push(`\nYou are speaking directly with the operator — your human principal — in the operator chat.`);
  } else {
    parts.push(
      `\nThis run was triggered by automation (API or cron), not live chat. Produce a complete, self-contained skill report.`,
    );
  }

  if (skillSlug) {
    const skillDef = mySkills.find((s) => s.slug === skillSlug);
    parts.push(`\n## SKILL EXECUTION MODE`);
    parts.push(`Dispatched skill: **${skillSlug}**.`);
    if (skillDef?.description) {
      parts.push(`Skill purpose: ${skillDef.description}`);
    }

    const ctxData = taskCtx as { combined_system_addon?: string; context_notes?: unknown[] } | null;
    if (ctxData?.combined_system_addon) {
      parts.push(`\n### Task Context\n${ctxData.combined_system_addon.slice(0, 3000)}`);
    }
    if (Array.isArray(ctxData?.context_notes) && ctxData.context_notes.length > 0) {
      const noteLines = (ctxData.context_notes as Array<{ actor?: string; text?: string }>)
        .slice(-6)
        .map((n) => `[${n.actor ?? "?"}] ${(n.text ?? "").slice(0, 200)}`);
      parts.push(`\n### Recent context\n${noteLines.join("\n")}`);
    }

    parts.push(
      `\nExecute this skill now. Return a complete, structured output — as if you just ran the skill and are reporting the result. Be substantive, not conversational.`,
    );
  } else if (audience === "operator_chat") {
    parts.push(`Be direct, opinionated, and in-character. Refer to yourself by your role. No markdown headers.`);
  }

  if (teammates.length > 0) {
    parts.push(`\n## Your team\n${teammates.join("\n")}`);
  }

  if (mySkills.length > 0) {
    const skillLines = mySkills.map((s) => `- **${s.slug}**: ${s.description ?? ""}`).join("\n");
    parts.push(`\n## Your skills\n${skillLines}`);
  }

  if (memories.length > 0) {
    const memLines = memories
      .slice(0, 8)
      .map((m) => `- [${m.kind ?? "note"}] ${m.title ?? ""}: ${(m.content ?? "").slice(0, 120)}`)
      .join("\n");
    parts.push(`\n## Company memory (recent)\n${memLines}`);
  }

  return parts.join("\n");
}

export async function resolveAgentForPersona(
  companyId: string,
  persona: string,
): Promise<{
  agentRegistryId: string | undefined;
  mySkillSlugs: string[];
  allKnownSlugs: string[];
  agentAdapterConfig: Record<string, unknown> | null;
}> {
  const [agentsData, skillsData] = await Promise.all([
    safeFetch(`${UPSTREAM}/api/company/companies/${companyId}/agents`, 2000),
    safeFetch(`${UPSTREAM}/api/company/companies/${companyId}/skills`, 2000),
  ]);
  const agents: AgentRecord[] = (agentsData as { agents?: AgentRecord[] })?.agents ?? [];
  const me =
    agents.find(
      (a) =>
        a.agent_ref === persona ||
        a.title?.toLowerCase() === persona.toLowerCase() ||
        (a.adapter_config?.paperclip?.agent_dir ?? "").includes(persona),
    ) ?? agents.find((a) => a.title?.toLowerCase().includes(persona.toLowerCase()));

  const skills: SkillRecord[] = (skillsData as { skills?: SkillRecord[] })?.skills ?? [];
  const mySkillSlugs = me?.adapter_config?.paperclip?.skills ?? [];
  const allKnownSlugs = mySkillSlugs.length > 0 ? mySkillSlugs : skills.map((s) => s.slug);
  return {
    agentRegistryId: me?.id,
    mySkillSlugs,
    allKnownSlugs,
    agentAdapterConfig: (me?.adapter_config ?? null) as Record<string, unknown> | null,
  };
}

export type SkillRunResult =
  | { ok: true; reply: string; runId: string | null; context_notes?: unknown }
  | { ok: false; error: string; httpStatus: number; runId?: string | null };

export type WorkerDispatchResult =
  | {
      ok: true;
      runId: string | null;
      status: "running" | "success" | "error";
      executionMode: "pending" | "worker" | "llm_simulated";
      summary: string | null;
      finalized: boolean;
    }
  | { ok: false; error: string; httpStatus: number; runId?: string | null };

function toObject(v: unknown): Record<string, unknown> | null {
  return v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : null;
}

function asStringArray(v: unknown): string[] {
  if (!Array.isArray(v)) return [];
  return v.filter((x): x is string => typeof x === "string").map((x) => x.trim()).filter(Boolean);
}

export function deriveToolExecutionPolicy(
  adapterConfig: Record<string, unknown> | null | undefined,
): ToolExecutionPolicySnapshot {
  const cfg = adapterConfig ?? {};
  const paperclip = toObject(cfg.paperclip);
  const toolPolicy = toObject(cfg.tool_policy) ?? toObject(cfg.policy) ?? {};
  const network = toObject(toolPolicy.network_boundary) ?? {};
  const exfil = toObject(toolPolicy.exfiltration) ?? {};

  const allowedTools = [
    ...asStringArray(toolPolicy.allowed_tools),
    ...asStringArray(paperclip?.allowed_tools),
  ];
  const sandboxRaw = String(toolPolicy.sandbox_mode ?? "workspace_write").toLowerCase();
  const sandbox_mode: ToolExecutionPolicySnapshot["sandbox_mode"] =
    sandboxRaw === "observe" || sandboxRaw === "capability_wasm" || sandboxRaw === "workspace_write"
      ? (sandboxRaw as ToolExecutionPolicySnapshot["sandbox_mode"])
      : "workspace_write";

  const blockNetRaw = network.block_network_for_bash;
  const block_network_for_bash =
    typeof blockNetRaw === "boolean"
      ? blockNetRaw
      : String(toolPolicy.network_mode ?? "").toLowerCase() === "deny";

  return {
    sandbox_mode,
    allowed_tools: Array.from(new Set(allowedTools)),
    network_boundary: {
      allowed_hosts: asStringArray(network.allowed_hosts),
      block_network_for_bash,
    },
    exfiltration: {
      enabled: exfil.enabled !== false,
      max_output_chars:
        typeof exfil.max_output_chars === "number" && Number.isFinite(exfil.max_output_chars)
          ? Math.max(256, Math.floor(exfil.max_output_chars))
          : 10_000,
    },
  };
}

export async function buildCompactedContextBundle(params: {
  companyId: string;
  taskId: string;
  agentRegistryId?: string;
  budgetBytes?: number;
}): Promise<CompactedContextBundle> {
  const { companyId, taskId, agentRegistryId, budgetBytes = 5200 } = params;
  const [taskCtxData, threadData, memoryData] = await Promise.all([
    safeFetch(`${UPSTREAM}/api/company/tasks/${taskId}/llm-context`, 5000),
    agentRegistryId
      ? safeFetch(`${UPSTREAM}/api/company/companies/${companyId}/agents/${agentRegistryId}/operator-thread`, 5000)
      : Promise.resolve(null),
    safeFetch(`${UPSTREAM}/api/company/companies/${companyId}/memory`, 5000),
  ]);

  const compactDigest = (threadData as { compact_digest?: string } | null)?.compact_digest?.trim() ?? "";
  const llmAddon = (taskCtxData as { combined_system_addon?: string } | null)?.combined_system_addon?.trim() ?? "";
  const memoryLines = ((memoryData as { memories?: Array<{ title?: string; content?: string; kind?: string }> } | null)?.memories ?? [])
    .slice(0, 6)
    .map((m) => `- [${m.kind ?? "note"}] ${m.title ?? ""}: ${(m.content ?? "").slice(0, 180)}`)
    .filter((s) => s.trim().length > 0)
    .join("\n");

  const sections: Array<{ name: string; text: string; tier: 0 | 1 | 2; cap: number }> = [];
  if (compactDigest) sections.push({ name: "operator_thread", text: compactDigest, tier: 0, cap: Math.floor(budgetBytes * 0.38) });
  if (llmAddon) sections.push({ name: "task_llm_context", text: llmAddon, tier: 1, cap: Math.floor(budgetBytes * 0.44) });
  if (memoryLines) sections.push({ name: "company_memory_recent", text: memoryLines, tier: 2, cap: Math.floor(budgetBytes * 0.18) });

  let used = 0;
  const out: string[] = [];
  const stats: Array<{ name: string; bytes: number; tier: 0 | 1 | 2 }> = [];
  for (const s of sections) {
    const header = `## ${s.name}\n`;
    const room = Math.max(0, Math.min(s.cap, budgetBytes - used - header.length - 2));
    if (room <= 0) break;
    const text = s.text.slice(0, room);
    const block = `${header}${text}\n`;
    out.push(block);
    used += block.length;
    stats.push({ name: s.name, bytes: block.length, tier: s.tier });
  }
  return {
    compactText: out.join("\n"),
    bytes: used,
    sections: stats,
  };
}

export async function upsertThreadSessionState(params: {
  companyId: string;
  persona: string;
  taskId: string;
  runId?: string | null;
  state: Record<string, unknown>;
}): Promise<void> {
  const { companyId, persona, taskId, runId, state } = params;
  const sessionKey = `${persona}:${taskId}`.toLowerCase();
  await fetch(`${UPSTREAM}/api/company/companies/${companyId}/thread-sessions`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      session_key: sessionKey,
      title: `${persona} · ${taskId.slice(0, 8)}`,
      participants: ["operator", persona],
      state: { ...state, run_id: runId ?? null, updated_at: new Date().toISOString() },
      is_active: true,
      created_by: "operator_chat",
    }),
  }).catch(() => {});
  await fetch(`${UPSTREAM}/api/company/companies/${companyId}/thread-sessions/${encodeURIComponent(sessionKey)}/join`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ participant: persona }),
  }).catch(() => {});
}

async function patchRunMetaExecutionMode(
  companyId: string,
  runId: string,
  meta: Record<string, unknown> | undefined,
  executionMode: "pending" | "worker" | "llm_simulated",
): Promise<void> {
  await fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${runId}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      meta: { ...(meta ?? {}), execution_mode: executionMode },
    }),
  });
}

/**
 * Worker-first dispatch for skill execution (checkout path), with optional
 * telemetry-based finalization of `agent_runs`.
 */
export async function dispatchSkillToWorker(params: {
  companyId: string;
  taskId: string;
  persona: string;
  skillSlug: string;
  externalSystem?: string;
  externalRunId?: string;
  persistAgentNote?: boolean;
  waitForTelemetryMs?: number;
  runSummary?: string;
  extraMeta?: Record<string, unknown>;
  dispatchNoteText?: string;
}): Promise<WorkerDispatchResult> {
  const {
    companyId,
    taskId,
    persona,
    skillSlug,
    externalSystem = "skill-run-api",
    externalRunId,
    persistAgentNote = true,
    waitForTelemetryMs = 15_000,
    runSummary,
    extraMeta,
    dispatchNoteText,
  } = params;

  const { agentRegistryId } = await resolveAgentForPersona(companyId, persona);
  const runId = await createAgentRun(companyId, agentRegistryId, taskId, skillSlug, {
    externalSystem,
    externalRunId,
    summary: runSummary ?? `Skill dispatched to worker (${externalSystem}): ${skillSlug}`,
    executionMode: "pending",
  });
  if (!runId) {
    return { ok: false, error: "Failed to create agent run", httpStatus: 502 };
  }

  if (extraMeta && Object.keys(extraMeta).length > 0) {
    await fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${runId}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ meta: { skill: skillSlug, triggered_by: externalSystem, execution_mode: "pending", ...extraMeta } }),
    }).catch(() => {});
  }

  const checkoutRes = await fetch(`${UPSTREAM}/api/company/tasks/${taskId}/checkout`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ agent_ref: persona, ttl_sec: 3600 }),
  });
  const checkoutJ = (await checkoutRes.json().catch(() => ({}))) as { error?: string };
  if (!checkoutRes.ok) {
    await finalizeAgentRun(
      companyId,
      runId,
      `Worker dispatch failed: ${checkoutJ.error ?? checkoutRes.statusText}`,
      "error",
    );
    return {
      ok: false,
      error: checkoutJ.error ?? `Worker dispatch failed (${checkoutRes.status})`,
      httpStatus: 502,
      runId,
    };
  }

  if (persistAgentNote) {
    await fetch(`${UPSTREAM}/api/company/tasks/${taskId}/stigmergic-note`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        text: dispatchNoteText ?? `Dispatched skill \`${skillSlug}\` to worker runtime.`,
        actor: persona,
      }),
    }).catch(() => {});
  }
  // Integrate existing coordinator path: allow spawn-rules to fan out background subtasks.
  await fetch(`${UPSTREAM}/api/company/companies/${companyId}/tasks/${taskId}/spawn-subagents`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ actor: persona, reason: "operator-chat-dispatch" }),
  }).catch(() => {});

  const endAt = Date.now() + Math.max(0, waitForTelemetryMs);
  let latestSummary: string | null = null;
  let latestMode: "pending" | "worker" | "llm_simulated" = "pending";

  while (Date.now() < endAt) {
    const [runRes, tasksRes] = await Promise.all([
      fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${runId}`),
      fetch(`${UPSTREAM}/api/company/companies/${companyId}/tasks`),
    ]);
    if (!runRes.ok || !tasksRes.ok) {
      await new Promise((r) => setTimeout(r, 2000));
      continue;
    }

    const runJson = (await runRes.json()) as {
      run?: { status?: string; summary?: string | null; meta?: Record<string, unknown> };
    };
    const tasksJson = (await tasksRes.json()) as {
      tasks?: Array<{ id: string; run?: { status?: string; tool_calls?: number; log_tail?: string } | null }>;
    };
    const task = (tasksJson.tasks ?? []).find((t) => t.id === taskId);
    const taskRunStatus = (task?.run?.status ?? "").toLowerCase();
    const taskToolCalls = task?.run?.tool_calls ?? 0;
    const observedWorker = taskToolCalls > 0;

    latestMode = observedWorker ? "worker" : latestMode;
    if (observedWorker && runJson.run?.meta?.execution_mode !== "worker") {
      await patchRunMetaExecutionMode(companyId, runId, runJson.run?.meta, "worker");
    }

    if (taskRunStatus === "success" || taskRunStatus === "error") {
      const finalMode = observedWorker ? "worker" : "llm_simulated";
      latestSummary =
        runJson.run?.summary?.trim() ||
        (typeof task?.run?.log_tail === "string" && task.run.log_tail.trim()
          ? task.run.log_tail.slice(-500)
          : `Task runtime ${taskRunStatus} (${taskToolCalls} tool calls)`);
      await fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${runId}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          status: taskRunStatus,
          summary: latestSummary,
          finished_at: true,
          meta: { ...(runJson.run?.meta ?? {}), execution_mode: finalMode },
        }),
      });
      return {
        ok: true,
        runId,
        status: taskRunStatus as "success" | "error",
        executionMode: finalMode,
        summary: latestSummary,
        finalized: true,
      };
    }

    await new Promise((r) => setTimeout(r, 2500));
  }

  return {
    ok: true,
    runId,
    status: "running",
    executionMode: latestMode,
    summary: latestSummary,
    finalized: false,
  };
}

/**
 * Create agent-run, run LLM in skill mode, finalize run, optionally append stigmergic note.
 */
export async function executeSkillLlmFlow(params: {
  companyId: string;
  taskId: string;
  persona: string;
  skillSlug: string;
  openRouterKey: string;
  externalSystem?: string;
  externalRunId?: string;
  audience?: PromptAudience;
  userMessage: string;
  persistAgentNote: boolean;
}): Promise<SkillRunResult> {
  const {
    companyId,
    taskId,
    persona,
    skillSlug,
    openRouterKey,
    externalSystem = "skill-run-api",
    externalRunId,
    audience = "headless",
    userMessage,
    persistAgentNote,
  } = params;

  const { agentRegistryId } = await resolveAgentForPersona(companyId, persona);

  const runId = await createAgentRun(companyId, agentRegistryId, taskId, skillSlug, {
    externalSystem,
    externalRunId,
    summary: `Skill run (${externalSystem}): ${skillSlug}`,
    executionMode: "llm_simulated",
  });

  const system = await Promise.race([
    buildSystemPrompt(persona, companyId, skillSlug, taskId, audience),
    new Promise<string>((resolve) =>
      setTimeout(() => resolve(`You are ${persona}, an AI agent. Execute the skill and report results.`), 7000),
    ),
  ]);

  const llmRes = await fetch(`${OR_BASE}/chat/completions`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${openRouterKey}`,
      "Content-Type": "application/json",
      "HTTP-Referer": "https://hsm.ai",
      "X-Title": "HSM Company Console",
    },
    body: JSON.stringify({
      model: CHAT_MODEL,
      messages: [
        { role: "system", content: system },
        { role: "user", content: userMessage },
      ],
      max_tokens: 1024,
      temperature: 0.4,
    }),
  });

  if (!llmRes.ok) {
    const errText = await llmRes.text().catch(() => llmRes.statusText);
    if (runId) await finalizeAgentRun(companyId, runId, `LLM error: ${errText}`, "error");
    return { ok: false, error: `LLM ${llmRes.status}: ${errText}`, httpStatus: 502, runId };
  }

  const data = (await llmRes.json()) as {
    choices?: Array<{ message?: { content?: string } }>;
    error?: { message?: string };
  };

  if (data.error) {
    if (runId) await finalizeAgentRun(companyId, runId, data.error.message ?? "LLM error", "error");
    return { ok: false, error: data.error.message ?? "LLM error", httpStatus: 502, runId };
  }

  const reply = data.choices?.[0]?.message?.content?.trim() ?? "";
  if (!reply) {
    if (runId) await finalizeAgentRun(companyId, runId, "Empty LLM response", "error");
    return { ok: false, error: "Empty response from LLM", httpStatus: 502, runId };
  }

  if (runId) {
    await finalizeAgentRun(companyId, runId, reply.slice(0, 1000), "success");
  }

  if (persistAgentNote) {
    const noteRes = await fetch(`${UPSTREAM}/api/company/tasks/${taskId}/stigmergic-note`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ text: reply, actor: persona }),
    });
    const noteData = (await noteRes.json().catch(() => ({}))) as {
      context_notes?: unknown;
      error?: string;
    };
    return { ok: true, reply, runId, context_notes: noteData.context_notes };
  }

  return { ok: true, reply, runId };
}
