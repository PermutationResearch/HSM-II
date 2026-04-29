/**
 * Shared server-only helpers for operator chat + skill execution (OpenRouter + Company OS).
 */

import { execFile } from "node:child_process";
import { canTransitionRunLoopState, type RunLoopState } from "@/app/lib/runtime-contract";
import { extractReplyFromChatCompletionPayload } from "@/app/lib/llm-text-content";

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
  const t = m.trim();
  if (!t.startsWith("openrouter/")) return t;
  const rest = t.slice("openrouter/".length);
  // Keep OpenRouter-native ids like `openrouter/elephant-alpha`.
  // Strip only wrapper ids like `openrouter/openai/gpt-4o`.
  return rest.includes("/") ? rest : t;
}

const DEFAULT_MODEL = "openrouter/elephant-alpha";

export const CHAT_MODEL = normalizeModel(
  process.env.HSM_AGENT_CHAT_MODEL ?? process.env.DEFAULT_LLM_MODEL ?? DEFAULT_MODEL,
);

/**
 * Shared harness contract addendum used by worker-sidecar prompts.
 * Kept exported so stream routes can import without duplicating policy text.
 */
export function companyOsHarnessAddendum(mode: "default" | "companion" = "default"): string {
  const core = [
    "You are operating inside the Company OS harness.",
    "Use tools for actionable repo/workspace requests.",
    "Keep responses concise and grounded in tool output.",
  ];
  if (mode === "companion") {
    core.push(
      "You are a sidecar stream: summarize intent/progress briefly while runtime tool events stream separately.",
    );
  }
  return core.join("\n");
}

export type StigNote = { at: string; actor: string; text: string };

export type OptimizeCommand =
  | { kind: "plan"; stepIndex: number }
  | { kind: "signature"; signatureName: string }
  | { kind: "task" };

const OLLAMA_BASE = (process.env.OLLAMA_URL ?? process.env.OLLAMA_HOST ?? "http://127.0.0.1:11434").replace(
  /\/+$/,
  "",
);
const DEFAULT_OLLAMA_MODEL = "llama3.2";

export type AgentChatBackend =
  | { provider: "openrouter"; label: "openrouter"; model: string; baseUrl: string; apiKey: string }
  | { provider: "ollama"; label: "ollama"; model: string; baseUrl: string };

/** Resolve conversational backend for Next operator chat routes. */
export function readAgentChatBackend(): AgentChatBackend | null {
  const provider = (process.env.HSM_AGENT_CHAT_PROVIDER ?? "").trim().toLowerCase();
  const openRouterKey = readOpenRouterApiKey();
  if (provider === "openrouter") {
    return openRouterKey
      ? {
          provider: "openrouter",
          label: "openrouter",
          model: CHAT_MODEL,
          baseUrl: OR_BASE,
          apiKey: openRouterKey,
        }
      : null;
  }
  if (provider === "ollama") {
    return {
      provider: "ollama",
      label: "ollama",
      model: process.env.HSM_AGENT_CHAT_MODEL ?? process.env.OLLAMA_MODEL ?? DEFAULT_OLLAMA_MODEL,
      baseUrl: OLLAMA_BASE,
    };
  }
  if (openRouterKey) {
    return {
      provider: "openrouter",
      label: "openrouter",
      model: CHAT_MODEL,
      baseUrl: OR_BASE,
      apiKey: openRouterKey,
    };
  }
  return {
    provider: "ollama",
    label: "ollama",
    model: process.env.HSM_AGENT_CHAT_MODEL ?? process.env.OLLAMA_MODEL ?? DEFAULT_OLLAMA_MODEL,
    baseUrl: OLLAMA_BASE,
  };
}

export function looksLikeExecutionIntent(text: string): boolean {
  const t = text.trim().toLowerCase();
  if (!t) return false;
  // Strong signal: message starts with an action verb (imperative).
  if (
    /^(please\s+)?(run|do|execute|fix|implement|build|search|grep|read|edit|write|analyze|check|verify|look|find|show|inspect|test|deploy|review|validate|list|create|delete|remove|refactor|debug|investigate|fetch|pull|push|diff|lint|format|compile|install|update|patch)\b/.test(
      t,
    )
  )
    return true;
  // Weaker signal: contains an action verb paired with a tool-work object anywhere in the message.
  const hasToolVerb =
    /\b(check|verify|inspect|validate|confirm|investigate|look\s+(?:at|into|for)|find\s+(?:all|where|any|the)|show\s+(?:me|the))\b/i.test(
      t,
    );
  const hasToolObject =
    /\b(test|tests|build|error|bug|issue|file|repo|code|workspace|log|output|result|function|method|class|module|route|endpoint|migration|schema)\b/i.test(
      t,
    );
  return hasToolVerb && hasToolObject;
}

export const QUICK_TOOL_PATH_SEGMENT_ROOTS =
  "src|apps|web|crates|packages|lib|tests?|scripts|\\.github|docs|migrations|app|components|pages|api|server|infra|tools|skills|e2e|playwright|benchmarks|examples|contracts|proto|internal|pkg|cmd";

export function looksLikeCodingToolIntentBody(text: string): boolean {
  const t = text.trim();
  if (!t) return false;
  if (/```/.test(text)) return true;
  if (/`[^`\n]{4,200}`/.test(text)) return true;
  if (/\b(src\/|apps\/|web\/|crates\/|packages\/|lib\/|tests?\/|scripts\/|\.github\/)\S+/i.test(text)) return true;
  if (/\S+\.(ts|tsx|mts|cts|js|jsx|mjs|cjs|rs|go|py|toml|json|ya?ml|md|sh)\b/i.test(text)) return true;
  if (/\b(cargo|pnpm|npm|yarn|npx|bun|git|make|cmake|pytest|jest|vitest|deno|docker|kubectl)\s+/i.test(text)) return true;
  if (/\b(Cargo\.toml|package\.json|pnpm-lock|Dockerfile|go\.mod)\b/i.test(text)) return true;
  if (/\b(bug|stack\s*trace|TypeError|panic|undefined is not|failing test|test failed)\b/i.test(text)) return true;
  return false;
}

export function looksLikeCodingToolIntent(text: string): boolean {
  return looksLikeExecutionIntent(text) || looksLikeCodingToolIntentBody(text);
}

/** Very short social replies — keep on conversational LLM when worker-first mode is on. */
export function looksPurelyConversationalChitChat(text: string): boolean {
  const t = text.trim();
  if (t.length > 120) return false;
  if (/[/`]/.test(t)) return false;
  if (/\.\w{1,6}\b/.test(t)) return false;
  // Require the greeting/ack word fills the WHOLE message — "ok, please fix the bug" is not chitchat.
  return (
    /^(hi|hello|hey|thanks|thank you|thx|ok+|okay|yes|no|bye|good morning|good night)[\s!.?,]*$/i.test(t) || t.length <= 4
  );
}

export function looksLikeQuickToolIntent(text: string): boolean {
  if (process.env.HSM_OPERATOR_CHAT_DISABLE_QUICK_TOOL === "1") return false;
  const t = text.trim();
  if (!t || t.length > 12_000) return false;
  if (looksPurelyConversationalChitChat(text)) return false;
  if (/```/.test(text)) return false;
  const ext =
    "ts|tsx|mts|cts|js|jsx|mjs|cjs|rs|go|py|toml|json|ya?ml|md|sh|html|htm|css|scss|less|sql|graphql|vue|svelte|kt|kts|java|rb|ex|exs|swift|cpp|hpp|cc|hh|c|h|zsh|fish|ps1|lock|wasm|wgsl|txt|ini|nix|bazel|bzl|gradle";
  const seg = QUICK_TOOL_PATH_SEGMENT_ROOTS;
  const segmentPath = new RegExp(`\\b(?:${seg})\\/\\S+`, "i");
  const relDotPath = /(?:^|[\s([{<,])\.{1,2}\/[\w./-]+/i.test(t);
  const deepRelFile = new RegExp(`[\\w.-]+(?:\\/[\\w.-]+)+\\.(?:${ext})\\b`, "i");
  const backtickPathLike = /`[^`\n]*(?:\/|\.\/|\.\.\/)[^`\n]{1,240}`/i.test(t);
  const backtickFileish = new RegExp("`[^`\\n]{1,240}\\.(?:" + ext + ")\\b[^`\\n]{0,40}`", "i").test(t);
  const fileWithExt = new RegExp(`\\b[\\w./-]*[\\w.-]+\\.(?:${ext})\\b`, "i");
  const hasPathLike =
    segmentPath.test(t) ||
    relDotPath ||
    deepRelFile.test(t) ||
    /\bCargo\.toml\b|\bpackage\.json\b|\bpnpm-lock\.yaml\b|\bDockerfile\b|\bgo\.mod\b|\bflake\.nix\b|\b(?:WORKSPACE|MODULE\.bazel)\b/i.test(
      t,
    ) ||
    backtickPathLike ||
    backtickFileish ||
    fileWithExt.test(t) ||
    /`[^`\n]{2,260}`/.test(t);
  if (!hasPathLike) return false;
  const readPhrasing =
    /\b(can you|could you|please)\s+(read|open|show|view|display|print|peek at|pull up|grab|load)\b/i.test(t) ||
    /\b(show me|let me see|i(?:'d)?\s+like to see|i need to see|want to see|take a look at)\b/i.test(t) ||
    /\b(what'?s?\s+in|what is in|contents of|content of|look inside|open up|snippet from|paste)\b/i.test(t) ||
    /\b(read|open|view|show)\s+[`'"]?[\w./-]/i.test(t) ||
    /\b(head|tail)\s+[`'"]?[\w./-]/i.test(t) ||
    /\b(grep|rg)\b.{0,160}\bin\b/i.test(t);
  const editPhrasing =
    /\b(can you|could you|please)\s+(change|update|edit|modify|patch|fix|replace|rename)\b/i.test(t) ||
    new RegExp(
      "\\b(change|update|edit|modify|replace|append to|delete|remove from)\\s+.{0,140}(`[^`]{2,200}`|\\./|(?:src|web|apps|lib|tests?|app|components)/|\\S+\\.(?:" +
        ext +
        ")\\b)",
      "i",
    ).test(t);
  return readPhrasing || editPhrasing;
}

export function looksLikeSkillsOrCatalogQuestion(text: string): boolean {
  const t = text.trim();
  if (!t || t.length > 8000) return false;
  if (looksPurelyConversationalChitChat(text)) return false;
  const mentionsSkills =
    /\bskills?\b/i.test(t) ||
    /\bSKILL\.md\b/i.test(t) ||
    /\bskill_md_read\b/i.test(t) ||
    /\bskills_list\b/i.test(t);
  if (!mentionsSkills) return false;
  return (
    /\b(what'?s?\s+in|what\s+is\s+in|tell me|show me|describe|list|contents?|catalog)\b/i.test(t) ||
    /\bwhat\b/i.test(t) || // "what skills does the company have?" — bare 'what' + mentionsSkills is enough
    /\b(integrat|added|merged|paperclip|on[- ]disk|repo)\b/i.test(t) ||
    /\b(can you|could you|please)\b/i.test(t)
  );
}

export function looksLikeImplicitWorkspacePointer(text: string): boolean {
  const t = text.trim();
  if (!t || t.length > 600) return false;
  if (looksPurelyConversationalChitChat(text)) return false;
  if (/\b(?:run|execute|dispatch)\s+/.test(t)) return false;
  return (
    /\b(?:it'?s|its)\s+in\s+(?:the\s+)?(?:files?|file|repo|repository|workspace|codebase|tree)\b/i.test(t) ||
    /\bin\s+(?:the\s+)?(?:files?|repo|repository|workspace|codebase)\s+(?:here|already|now)\b/i.test(t) ||
    /\blook\s+(?:in|at)\s+(?:the\s+)?(?:files?|repo|workspace)\b/i.test(t) ||
    /\b(?:on|under)\s+dis(?:k|c)\b/i.test(t) ||
    /\bfiles\b.*\bcompany\b/i.test(t) ||
    /\bwhat(?:'s|s|\s+are)\s+(?:the\s+)?files\b.*\bcompany\b/i.test(t) ||
    /\bwhat\s+files\b.*\b(?:company|repo|workspace|codebase)\b/i.test(t) ||
    /\bwhich\s+files\b.*\b(?:company|repo|workspace)\b/i.test(t) ||
    /\b(list|enumerate|show)\s+(?:me\s+)?(?:all\s+)?(?:the\s+)?(?:files|directories|folders)\b.*\b(?:company|repo|workspace)\b/i.test(t)
  );
}

export function looksLikeCapabilityQuestion(text: string): boolean {
  const t = text.trim();
  if (!t || t.length > 500) return false;
  const asksCapability =
    /\b(can you|could you|you can|are you able to|do you have access to|are you allowed to)\b/i.test(t) ||
    /\bdo you (?:read|access|see|open|modify|edit|write)\b/i.test(t);
  if (!asksCapability) return false;
  const mentionsResources =
    /\b(file|files|repo|repository|workspace|directory|folder|codebase|internal|company)\b/i.test(t);
  if (!mentionsResources) return false;
  const asksConcreteExecution =
    /\b(please|go ahead|now|right now)\b/i.test(t) ||
    /\b(read|open|show|list|edit|write|update|patch)\b.{0,140}\b(?:`[^`]+`|\.{1,2}\/|src\/|web\/|app\/|Cargo\.toml|package\.json)\b/i.test(
      t,
    );
  return !asksConcreteExecution;
}

export function looksLikeRepoInfoQuestion(text: string): boolean {
  const t = text.trim();
  if (!t || t.length > 800) return false;
  if (looksPurelyConversationalChitChat(text)) return false;
  if (looksLikeCapabilityQuestion(text)) return false;
  const asksForInfo = /\b(what(?:'s| is| are)?|which|show|list|enumerate|where)\b/i.test(t) || /\?$/.test(t);
  if (!asksForInfo) return false;
  const mentionsWorkObject =
    /\b(file|files|repo|repository|workspace|codebase|directory|directories|folder|folders|tree|skill|skills)\b/i.test(
      t,
    );
  if (!mentionsWorkObject) return false;
  const impliesLocalContext =
    /\b(internal|company|here|in (?:the )?(?:repo|repository|workspace|project|company)|of (?:the )?company)\b/i.test(
      t,
    );
  return impliesLocalContext;
}

/**
 * Technical/code questions that need workspace context to answer properly.
 * These aren't imperative commands, but the worker should still handle them
 * so it can read relevant files rather than hallucinating from training data.
 *
 * Examples that match:
 *   "What does the SFT module do?"
 *   "How does the execute-worker path work?"
 *   "Why is the routing split into two files?"
 *   "Explain the Hermes tool loop"
 *   "Walk me through the agent loop"
 */
export function looksLikeTechnicalQuestion(text: string): boolean {
  const t = text.trim();
  if (!t || t.length < 15) return false;
  if (looksPurelyConversationalChitChat(text)) return false;
  if (looksLikeCapabilityQuestion(text)) return false;
  // Must be question-shaped (ends with ? or starts with interrogative / explain verb)
  const isQuestion =
    /[?]$/.test(t) ||
    /^(what|how|why|where|which|when|explain|describe|tell me(?:\s+about)?|walk me through|show me how|help me understand)\b/i.test(
      t,
    );
  if (!isQuestion) return false;
  // Must mention something code/project-specific so generic curiosity ("what time is it?") doesn't match.
  return /\b(module|function|fn|method|class|struct|trait|impl|enum|type|interface|route|endpoint|schema|config|service|handler|middleware|guard|hook|component|util|helper|macro|plugin|provider|action|evaluator|worker|agent|loop|pipeline|trace|capture|sft|hermes|skill|tool|crate|lib|src|api|auth|token|session|db|database|query|migration|test|build|compile|deploy|task|persona|note|memory|workspace|repo|codebase|execute|dispatch|routing|streaming|agentic|runtime|harness)\b/i.test(
    t,
  );
}

export function looksLikeActionableWorkTurn(text: string): boolean {
  const t = text.trim();
  if (!t) return false;
  if (looksPurelyConversationalChitChat(text)) return false;
  if (looksLikeCapabilityQuestion(text)) return false;
  if (looksLikeRepoInfoQuestion(text)) return true;
  if (looksLikeCodingToolIntent(text)) return true;
  if (looksLikeQuickToolIntent(text)) return true;
  if (looksLikeSkillsOrCatalogQuestion(text)) return true;
  if (looksLikeImplicitWorkspacePointer(text)) return true;
  if (looksLikeTechnicalQuestion(text)) return true;
  const hasActionVerb =
    /\b(run|execute|fix|implement|build|debug|refactor|analy(?:s|z)e|investigate|inspect|open|read|show|list|edit|write|update|patch|create|delete|remove)\b/i.test(
      t,
    );
  const hasWorkObject =
    /\b(file|files|repo|repository|workspace|code|task|issue|bug|test|tests|build|command|terminal|directory|folder|project|skill|skills|tool|tools)\b/i.test(
      t,
    );
  return hasActionVerb && hasWorkObject;
}

export function operatorChatShouldRouteWorker(params: {
  lastOperatorText: string;
  hasChatBackend: boolean;
}): {
  routeWorker: boolean;
  executionIntent: boolean;
  codingIntent: boolean;
  quickToolIntent: boolean;
  reason: string;
} {
  const { lastOperatorText, hasChatBackend } = params;
  const quickToolIntent = looksLikeQuickToolIntent(lastOperatorText);
  const skillsCatalogQuestion = looksLikeSkillsOrCatalogQuestion(lastOperatorText);
  const implicitWorkspace = looksLikeImplicitWorkspacePointer(lastOperatorText);
  const actionableWork = looksLikeActionableWorkTurn(lastOperatorText);
  const codingFromSignals =
    looksLikeCodingToolIntent(lastOperatorText) || skillsCatalogQuestion || implicitWorkspace;
  const codingIntent = codingFromSignals || quickToolIntent;
  const executionIntent = looksLikeExecutionIntent(lastOperatorText) || codingIntent;

  if (process.env.HSM_FORCE_OPERATOR_WORKER_DISPATCH === "1") {
    return {
      routeWorker: true,
      executionIntent: looksLikeExecutionIntent(lastOperatorText) || codingIntent,
      codingIntent,
      quickToolIntent,
      reason: "forced_worker_dispatch",
    };
  }
  const workerFirst =
    process.env.HSM_OPERATOR_CHAT_WORKER_FIRST === "1" ||
    process.env.HSM_OPERATOR_CLAUDE_CODE_MODE === "1";
  if (!hasChatBackend) {
    return {
      routeWorker: actionableWork,
      executionIntent,
      codingIntent,
      quickToolIntent,
      reason: actionableWork ? "no_chat_backend_actionable" : "no_chat_backend_conversational",
    };
  }
  if (workerFirst && actionableWork) {
    return {
      routeWorker: true,
      executionIntent,
      codingIntent,
      quickToolIntent,
      reason: "worker_first_claude_code_mode",
    };
  }
  if (executionIntent) {
    const quickOnly =
      quickToolIntent &&
      !looksLikeCodingToolIntentBody(lastOperatorText) &&
      !looksLikeExecutionIntent(lastOperatorText);
    return {
      routeWorker: true,
      executionIntent,
      codingIntent,
      quickToolIntent,
      reason: quickOnly ? "quick_tool_read_edit" : "execution_or_coding_intent",
    };
  }
  // Actionable work (verb + work-object match, technical question, file signal, etc.)
  // routes to the worker even when a conversational backend is available.
  if (actionableWork) {
    return {
      routeWorker: true,
      executionIntent,
      codingIntent,
      quickToolIntent,
      reason: "actionable_work_turn",
    };
  }

  // ── Default: route non-trivial turns to the Hermes worker ─────────────────
  //
  // The worker can answer conversationally too — it has full system-prompt,
  // task context, memory, AND workspace tools available.  Sending everything
  // non-trivial through Hermes avoids the case where the user asks a code or
  // architecture question and gets a stale LLM answer that can't read local files.
  //
  // "Trivial" = pure chitchat (greetings, one-word acks) or a capability question
  // ("can you read files?").  These don't need tools so LLM-direct is fine.
  //
  // Opt out with HSM_OPERATOR_CHAT_CONSERVATIVE_ROUTING=1 to restore the old
  // behaviour (only route when explicit execution/coding signals fire).
  if (process.env.HSM_OPERATOR_CHAT_CONSERVATIVE_ROUTING !== "1") {
    const trivial =
      looksPurelyConversationalChitChat(lastOperatorText) ||
      looksLikeCapabilityQuestion(lastOperatorText);
    if (!trivial) {
      return {
        routeWorker: true,
        executionIntent,
        codingIntent,
        quickToolIntent,
        reason: "non_trivial_default_worker",
      };
    }
  }

  return {
    routeWorker: false,
    executionIntent: false,
    codingIntent: false,
    quickToolIntent: false,
    reason: "conversational_chat",
  };
}

export function operatorChatQuickToolPromptMode(params: {
  quickToolIntent: boolean;
  routeReason: string;
}): boolean {
  if (!params.quickToolIntent) return false;
  if (
    params.routeReason === "worker_first_claude_code_mode" ||
    params.routeReason === "forced_worker_dispatch"
  ) {
    return false;
  }
  return true;
}

// ─── Task Management Action Lane ────────────────────────────────────────────
//
// Deterministic parser + executor for task-management intents so prompts like
// "create a follow-up task: …", "assign this to maya", "hand back / release"
// never fall through to conversational LLM behavior.

export type TaskActionKind =
  | "create_task"
  | "assign_task"
  | "hand_back"
  | "mark_done"
  | "add_note"
  | "requires_human";

export interface TaskAction {
  kind: TaskActionKind;
  /** Raw operator intent text (used for note body / task title fallback). */
  raw: string;
  /** For create_task: extracted title after the trigger phrase. */
  title?: string;
  /** For create_task: optional specification paragraph following the title. */
  spec?: string;
  /** For assign_task: persona/agent name the operator named. */
  targetPersona?: string;
  /** For add_note: body of the note. */
  noteBody?: string;
}

export interface TaskActionResult {
  ok: boolean;
  kind: TaskActionKind;
  taskId?: string;
  message: string;
  /** Machine-readable data returned from the API (task object, etc.). */
  data?: unknown;
}

/** Classify operator text as a deterministic task-management action, or return null. */
export function parseTaskManagementAction(text: string): TaskAction | null {
  const t = text.trim();
  if (!t || t.length > 2000) return null;

  // ── create task ──────────────────────────────────────────────────────────
  // "create a follow-up task: Implement XYZ"
  // "create task: ..." | "new task: ..." | "add task: ..." | "open task: ..."
  const createM =
    /^(?:please\s+)?(?:create|add|open|make|new)\s+(?:a\s+)?(?:follow[\s-]up\s+)?(?:company\s+|workspace\s+)?task(?:\s+titled)?[:\s]+(.+)/is.exec(t);
  if (createM) {
    const body = createM[1].trim();
    // First sentence / line is the title; remainder is spec
    const lineBreak = body.search(/[\n\r]/);
    const periodBreak = body.search(/\.\s+[A-Z]/);
    const splitAt =
      lineBreak > 10 ? lineBreak : periodBreak > 10 ? periodBreak + 1 : body.length;
    const title = body.slice(0, splitAt).trim().replace(/\.$/, "");
    const spec = body.slice(splitAt).trim();
    return { kind: "create_task", raw: t, title, spec: spec || undefined };
  }

  // ── assign task ──────────────────────────────────────────────────────────
  // "assign this to maya" | "assign to @kai" | "reassign to ..."
  const assignM =
    /^(?:please\s+)?(?:assign|re-?assign)\s+(?:this\s+)?(?:task\s+)?to\s+[@]?(\w[\w\s-]{0,40})/i.exec(t);
  if (assignM) {
    return { kind: "assign_task", raw: t, targetPersona: assignM[1].trim() };
  }

  // ── hand back / release ──────────────────────────────────────────────────
  // "hand back" | "hand this back" | "release task" | "release checkout"
  if (
    /^(?:please\s+)?(?:hand\s+(?:this\s+)?back|release\s+(?:this\s+)?(?:task|checkout)?)\b/i.test(t)
  ) {
    return { kind: "hand_back", raw: t };
  }

  // ── mark done ────────────────────────────────────────────────────────────
  // "mark done" | "mark this done" | "mark complete" | "close task"
  if (
    /^(?:please\s+)?(?:mark\s+(?:this\s+)?(?:done|complete[d]?)|close\s+(?:this\s+)?task|done\s+with\s+this)\b/i.test(t)
  ) {
    return { kind: "mark_done", raw: t };
  }

  // ── requires human ───────────────────────────────────────────────────────
  // "needs human" | "flag for human review" | "requires human"
  if (
    /^(?:please\s+)?(?:needs?\s+human|flag\s+(?:for\s+)?human|requires?\s+human|escalat[e]?\s+to\s+human)\b/i.test(t)
  ) {
    return { kind: "requires_human", raw: t };
  }

  // ── add note ─────────────────────────────────────────────────────────────
  // "add note: ..." | "note: ..." | "add a note: ..." | "log: ..."
  const noteM =
    /^(?:please\s+)?(?:add\s+(?:a\s+)?note|note|log)[:\s]+(.+)/is.exec(t);
  if (noteM) {
    return { kind: "add_note", raw: t, noteBody: noteM[1].trim() };
  }

  return null;
}

/** Execute a parsed task management action deterministically against Company OS APIs. */
export async function executeTaskAction(params: {
  action: TaskAction;
  companyId: string;
  taskId: string;
  persona: string;
}): Promise<TaskActionResult> {
  const { action, companyId, taskId, persona } = params;

  switch (action.kind) {
    case "create_task": {
      const title = action.title ?? action.raw.slice(0, 120);
      const body: Record<string, unknown> = {
        title,
        owner_persona: persona,
        priority: 0,
      };
      if (action.spec) body["specification"] = action.spec;
      const res = await fetch(
        `${UPSTREAM}/api/company/companies/${companyId}/tasks`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(body),
        },
      );
      const j = (await res.json().catch(() => ({}))) as { task?: { id?: string; display_number?: number } };
      if (!res.ok) {
        return { ok: false, kind: "create_task", message: (j as { error?: string }).error ?? `API error ${res.status}` };
      }
      const newId = j.task?.id;
      const num = j.task?.display_number;
      // Post a stigmergic note on the source task linking to the new one
      await fetch(`${UPSTREAM}/api/company/tasks/${taskId}/stigmergic-note`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          text: `Created follow-up task${num ? ` #${num}` : ""}${newId ? ` (${newId})` : ""}: **${title}**`,
          actor: persona,
        }),
      }).catch(() => {});
      return {
        ok: true,
        kind: "create_task",
        taskId: newId,
        message: `Created task${num ? ` #${num}` : ""}${newId ? ` \`${newId.slice(0, 8)}\`` : ""}: **${title}**`,
        data: j.task,
      };
    }

    case "assign_task": {
      const target = action.targetPersona ?? persona;
      const res = await fetch(
        `${UPSTREAM}/api/company/companies/${companyId}/tasks`,
      );
      if (!res.ok) {
        return { ok: false, kind: "assign_task", message: `Could not load task list (${res.status})` };
      }
      // PATCH the current task's owner_persona
      const patchRes = await fetch(
        `${UPSTREAM}/api/company/companies/${companyId}/tasks/${taskId}`,
        {
          method: "PATCH",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ owner_persona: target }),
        },
      );
      if (!patchRes.ok) {
        // Fallback: post a stigmergic handoff note
        await fetch(`${UPSTREAM}/api/company/tasks/${taskId}/stigmergic-note`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ text: `Assign to **${target}**: ${action.raw}`, actor: persona }),
        }).catch(() => {});
        return {
          ok: true,
          kind: "assign_task",
          taskId,
          message: `Logged assignment handoff to **${target}** as a task note (PATCH not available).`,
        };
      }
      await fetch(`${UPSTREAM}/api/company/tasks/${taskId}/stigmergic-note`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          text: `Task reassigned to **${target}** by ${persona}.`,
          actor: persona,
        }),
      }).catch(() => {});
      return { ok: true, kind: "assign_task", taskId, message: `Task assigned to **${target}**.`, data: { target } };
    }

    case "hand_back": {
      const releaseRes = await fetch(
        `${UPSTREAM}/api/company/tasks/${taskId}/release`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ actor: persona }),
        },
      );
      const note = releaseRes.ok
        ? `Task checkout released by ${persona}.`
        : `Release request noted (release endpoint returned ${releaseRes.status}).`;
      await fetch(`${UPSTREAM}/api/company/tasks/${taskId}/stigmergic-note`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ text: note, actor: persona }),
      }).catch(() => {});
      return { ok: true, kind: "hand_back", taskId, message: note };
    }

    case "mark_done": {
      const patchRes = await fetch(
        `${UPSTREAM}/api/company/companies/${companyId}/tasks/${taskId}`,
        {
          method: "PATCH",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ state: "done" }),
        },
      );
      const msg = patchRes.ok
        ? "Task marked as done."
        : `State update noted (PATCH returned ${patchRes.status}); adding completion note.`;
      await fetch(`${UPSTREAM}/api/company/tasks/${taskId}/stigmergic-note`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ text: `Marked done by ${persona}. ${action.raw}`, actor: persona }),
      }).catch(() => {});
      return { ok: true, kind: "mark_done", taskId, message: msg };
    }

    case "requires_human": {
      const rhRes = await fetch(
        `${UPSTREAM}/api/company/tasks/${taskId}/requires-human`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ requires_human: true, actor: persona, reason: action.raw }),
        },
      );
      const msg = rhRes.ok
        ? "Task flagged for human review."
        : `Human-review flag noted (API returned ${rhRes.status}); added task note.`;
      await fetch(`${UPSTREAM}/api/company/tasks/${taskId}/stigmergic-note`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          text: `Flagged for human review by ${persona}: ${action.raw}`,
          actor: persona,
        }),
      }).catch(() => {});
      return { ok: true, kind: "requires_human", taskId, message: msg };
    }

    case "add_note": {
      const body = action.noteBody ?? action.raw;
      const noteRes = await fetch(`${UPSTREAM}/api/company/tasks/${taskId}/stigmergic-note`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ text: body, actor: persona }),
      });
      return {
        ok: noteRes.ok,
        kind: "add_note",
        taskId,
        message: noteRes.ok ? `Note added to task.` : `Failed to add note (${noteRes.status}).`,
      };
    }

    default:
      return { ok: false, kind: action.kind, message: "Unknown action kind." };
  }
}

// ─── End Task Management Action Lane ────────────────────────────────────────

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

async function writeWorkspaceFile(companyId: string, path: string, content: string): Promise<boolean> {
  try {
    const res = await fetch(`${UPSTREAM}/api/company/companies/${companyId}/workspace/file`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ path, content }),
    });
    return res.ok;
  } catch {
    return false;
  }
}

function sanitizeRunSlug(text: string): string {
  const s = text
    .toLowerCase()
    .replace(/[^a-z0-9-_]+/g, "-")
    .replace(/-+/g, "-")
    .replace(/^-+|-+$/g, "");
  return s || "run";
}

async function ensureRecursiveDocsForRun(params: {
  companyId: string;
  runId: string;
  persona: string;
  skillSlug: string;
  taskId: string;
  userPrompt: string;
}): Promise<{ recursiveRunPath: string; recursiveReqPath: string } | null> {
  const { companyId, runId, persona, skillSlug, taskId, userPrompt } = params;
  const day = new Date().toISOString().slice(0, 10);
  const slug = sanitizeRunSlug(`${day}-${persona}-${skillSlug}-${runId.slice(0, 8)}`);
  const runPath = `.recursive/run/${slug}`;
  const reqPath = `${runPath}/00-requirements.md`;
  const asIsPath = `${runPath}/01-as-is.md`;
  const toBePath = `${runPath}/02-to-be.md`;
  const implPath = `${runPath}/03-implementation-summary.md`;
  const testPath = `${runPath}/04-test-summary.md`;
  const qaPath = `${runPath}/05-manual-qa.md`;

  const recursiveSpec = `# RECURSIVE (HSM adaptation)

This repository uses a file-backed run protocol for long-horizon agent work.

## Required phases per run
1. requirements (\`00-requirements.md\`)
2. as-is (\`01-as-is.md\`)
3. to-be (\`02-to-be.md\`)
4. implementation summary (\`03-implementation-summary.md\`)
5. test summary (\`04-test-summary.md\`)
6. manual QA (\`05-manual-qa.md\`)

## Exit criteria
- No run may be marked complete without execution evidence (\`execution_verified=true\`).
- Tool traces, touched files, and run artifacts must be persisted.
- Manual QA checklist must be present for user-facing changes.
`;

  const stateLine = `- ${new Date().toISOString()} run=${runId} task=${taskId} persona=${persona} skill=${skillSlug} path=${runPath}`;
  const existingState = (await fetchWorkspaceFile(companyId, ".recursive/STATE.md")) ?? "# STATE\n\n## Active runs\n";
  const stateContent = existingState.includes(stateLine) ? existingState : `${existingState.trimEnd()}\n${stateLine}\n`;

  const decisionLine = `- ${new Date().toISOString()}: run ${runId} uses recursive artifacts in \`${runPath}\` and requires worker evidence for completion.`;
  const existingDecisions =
    (await fetchWorkspaceFile(companyId, ".recursive/DECISIONS.md")) ?? "# DECISIONS\n\n## Ledger\n";
  const decisionsContent = existingDecisions.includes(decisionLine)
    ? existingDecisions
    : `${existingDecisions.trimEnd()}\n${decisionLine}\n`;

  const reqContent = `# Requirements

- run_id: ${runId}
- task_id: ${taskId}
- persona: ${persona}
- skill: ${skillSlug}

## Operator request
${userPrompt.trim() || "(empty prompt)"}
`;
  const asIsContent = `# As-Is

- run_id: ${runId}
- current execution mode: worker-dispatch
- source task: ${taskId}

## Current known constraints
- Worker run must emit tool evidence.
- Completion requires \`execution_verified=true\`.
`;
  const toBeContent = `# To-Be

## Planned implementation path
1. gather context
2. emit tool calls and execute
3. persist artifacts + touched files
4. validate outputs and tests
`;
  const implContent = `# Implementation Summary

_Populate during/after execution with concrete files and tool outputs._
`;
  const testContent = `# Test Summary

_Record automated checks and outcomes._
`;
  const qaContent = `# Manual QA

- [ ] Behavior verified by operator
- [ ] Artifacts visible in run panel
- [ ] Touched files open correctly
`;

  const writes = await Promise.all([
    writeWorkspaceFile(companyId, ".recursive/RECURSIVE.md", recursiveSpec),
    writeWorkspaceFile(companyId, ".recursive/memory/README.md", "# Recursive Memory\n\nDurable operational notes.\n"),
    writeWorkspaceFile(companyId, ".recursive/STATE.md", stateContent),
    writeWorkspaceFile(companyId, ".recursive/DECISIONS.md", decisionsContent),
    writeWorkspaceFile(companyId, reqPath, reqContent),
    writeWorkspaceFile(companyId, asIsPath, asIsContent),
    writeWorkspaceFile(companyId, toBePath, toBeContent),
    writeWorkspaceFile(companyId, implPath, implContent),
    writeWorkspaceFile(companyId, testPath, testContent),
    writeWorkspaceFile(companyId, qaPath, qaContent),
  ]);
  if (writes.some((ok) => !ok)) return null;
  return { recursiveRunPath: runPath, recursiveReqPath: reqPath };
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
      meta: { skill: skillSlug, triggered_by: external_system, execution_mode, execution_verified: false },
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

/**
 * Returns true when the model is a free-tier or known-small model that has a
 * tight context window (≤ 8K tokens).  In thin mode we compress the system
 * prompt so the model has real headroom for the conversation and its reply.
 *
 * Detects:
 *  - OpenRouter free-tier  (`:free` suffix, e.g. `deepseek/deepseek-r1:free`)
 *  - Ollama small models   (llama3.2, mistral:7b, phi3:mini, gemma:2b …)
 *  - Env override:         HSM_THIN_HARNESS=1 forces thin always
 *                          HSM_THIN_HARNESS=0 disables thin always
 */
export function isThinHarnessModel(modelId: string): boolean {
  const override = (process.env.HSM_THIN_HARNESS ?? "").trim();
  if (override === "1") return true;
  if (override === "0") return false;

  const m = modelId.toLowerCase();
  // OpenRouter free-tier suffix
  if (m.endsWith(":free")) return true;
  // Known small Ollama / generic models
  if (/\b(llama3(?:\.[12])?|llama3?[._:-]?[123]b?|phi[23]|gemma[._:-]?[27]b|mistral(?:[._:-]?7b)?|qwen[._:-]?[0-9]+b|smollm|tinyllama)\b/.test(m)) return true;
  return false;
}

export async function buildSystemPrompt(
  persona: string,
  companyId: string | undefined,
  skillSlug: string | null,
  taskId: string,
  audience: PromptAudience = "operator_chat",
  /** When true, caps every section to stay under ~1 K tokens total for free/small models. */
  thin = false,
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

  // Thin-mode limits — keep total system prompt under ~1 K tokens (~4 K chars)
  const visionMax  = thin ? 400  : 2000;
  const memMax     = thin ? 4    : 8;
  const memChars   = thin ? 80   : 120;
  const noteMax    = thin ? 3    : 6;
  const noteChars  = thin ? 120  : 200;
  const ctxMax     = thin ? 800  : 3000;
  const teamMax    = thin ? 3    : Infinity;

  const parts: string[] = [];

  if (me?.briefing) {
    // Thin: cap briefing at 300 chars so it doesn't crowd everything else
    const briefing = thin ? me.briefing.trim().slice(0, 300) : me.briefing.trim();
    parts.push(briefing);
  } else {
    const label = me?.title ?? persona;
    const roleStr = me?.role ? ` — ${me.role}` : "";
    parts.push(`You are ${label}${roleStr} at this company.`);
  }

  parts.push(`\nToday is ${now}.`);

  if (visionContent) {
    const snippet =
      visionContent.length > visionMax
        ? visionContent.slice(0, visionMax) + "\n…[truncated]"
        : visionContent;
    parts.push(`\n## Company Vision\n${snippet}`);
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
      parts.push(`\n### Task Context\n${ctxData.combined_system_addon.slice(0, ctxMax)}`);
    }
    if (Array.isArray(ctxData?.context_notes) && ctxData.context_notes.length > 0) {
      const noteLines = (ctxData.context_notes as Array<{ actor?: string; text?: string }>)
        .slice(-noteMax)
        .map((n) => `[${n.actor ?? "?"}] ${(n.text ?? "").slice(0, noteChars)}`);
      parts.push(`\n### Recent context\n${noteLines.join("\n")}`);
    }

    parts.push(
      `\nExecute this skill now. Return a complete, structured output — as if you just ran the skill and are reporting the result. Be substantive, not conversational.`,
    );
  } else if (audience === "operator_chat") {
    parts.push(`Be direct, opinionated, and in-character. Refer to yourself by your role. No markdown headers.`);
  }

  if (teammates.length > 0) {
    const team = teammates.slice(0, teamMax);
    const suffix = thin && teammates.length > teamMax ? ` (+${teammates.length - teamMax} more)` : "";
    parts.push(`\n## Your team\n${team.join("\n")}${suffix}`);
  }

  if (mySkills.length > 0) {
    const skillLines = thin
      ? mySkills.map((s) => `- ${s.slug}`).join(", ")                    // thin: names only
      : mySkills.map((s) => `- **${s.slug}**: ${s.description ?? ""}`).join("\n");
    const header = thin ? `\nYour skills: ` : `\n## Your skills\n`;
    parts.push(`${header}${skillLines}`);
  }

  if (memories.length > 0) {
    const memLines = memories
      .slice(0, memMax)
      .map((m) => `- [${m.kind ?? "note"}] ${m.title ?? ""}: ${(m.content ?? "").slice(0, memChars)}`)
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
      workerEvidence: boolean;
      executionVerified: boolean;
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
  executionVerified?: boolean,
): Promise<void> {
  await fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${runId}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      meta: {
        ...(meta ?? {}),
        execution_mode: executionMode,
        ...(typeof executionVerified === "boolean" ? { execution_verified: executionVerified } : {}),
      },
    }),
  });
}

async function checkoutTaskForWorker(taskId: string, persona: string): Promise<{ ok: boolean; error?: string; status: number }> {
  const checkoutRes = await fetch(`${UPSTREAM}/api/company/tasks/${taskId}/checkout`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ agent_ref: persona, ttl_sec: 3600 }),
  });
  const checkoutJ = (await checkoutRes.json().catch(() => ({}))) as { error?: string };
  if (!checkoutRes.ok) {
    return {
      ok: false,
      error: checkoutJ.error ?? checkoutRes.statusText,
      status: checkoutRes.status,
    };
  }
  return { ok: true, status: checkoutRes.status };
}

async function createFallbackDispatchTask(params: {
  companyId: string;
  persona: string;
  skillSlug: string;
  sourceTaskId: string;
  runSummary?: string;
}): Promise<string | null> {
  const { companyId, persona, skillSlug, sourceTaskId, runSummary } = params;
  const title = `${persona} worker turn (${skillSlug})`;
  const specification = [
    `Auto-created fallback task because checkout failed on source task ${sourceTaskId}.`,
    runSummary ? `Original summary: ${runSummary}` : "",
  ]
    .filter(Boolean)
    .join("\n");
  const res = await fetch(`${UPSTREAM}/api/company/companies/${companyId}/tasks`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      title,
      specification,
      owner_persona: persona,
      priority: 0,
    }),
  });
  if (!res.ok) return null;
  const j = (await res.json().catch(() => ({}))) as { task?: { id?: string } };
  return typeof j.task?.id === "string" ? j.task.id : null;
}

export function consoleUpstreamIsLoopback(): boolean {
  try {
    const u = new URL(UPSTREAM);
    const h = u.hostname.toLowerCase();
    return h === "localhost" || h === "127.0.0.1" || h === "::1" || h === "[::1]";
  } catch {
    return false;
  }
}

export function detectGitRepoRootForWorkspace(cwd = process.cwd()): Promise<string | null> {
  return new Promise((resolve) => {
    execFile("git", ["rev-parse", "--show-toplevel"], { cwd, timeout: 5000, maxBuffer: 4096 }, (err, stdout) => {
      if (err) return resolve(null);
      const p = stdout.toString().trim();
      resolve(p.length > 0 ? p : null);
    });
  });
}

function normalizeTaskWorkspacePaths(raw: unknown): string[] {
  if (!raw) return [];
  if (!Array.isArray(raw)) return [];
  return (raw as unknown[])
    .filter((x) => typeof x === "string" && (x as string).trim().length > 0)
    .map((x) => (x as string).trim());
}

export async function ensureTaskWorkspaceFromCompanyDefaults(params: {
  companyId: string;
  taskId: string;
}): Promise<{ patched: boolean; root?: string }> {
  const tasksJson = (await safeFetch(`${UPSTREAM}/api/company/companies/${params.companyId}/tasks`, 8000)) as {
    tasks?: Array<{ id: string; workspace_attachment_paths?: unknown }>;
  } | null;
  const task = tasksJson?.tasks?.find((x) => x.id === params.taskId);
  if (!task) return { patched: false };
  const paths = normalizeTaskWorkspacePaths(task.workspace_attachment_paths);
  if (paths.length > 0) return { patched: false };
  const companyJson = (await safeFetch(`${UPSTREAM}/api/company/companies/${params.companyId}`, 5000)) as {
    company?: { default_workspace_root?: string | null; hsmii_home?: string | null };
  } | null;
  const autoRepoOff = process.env.HSM_OPERATOR_CHAT_AUTO_REPO_WORKSPACE === "0";
  let root =
    (companyJson?.company?.default_workspace_root ?? "").trim() ||
    (companyJson?.company?.hsmii_home ?? "").trim();
  if (!root && !autoRepoOff && consoleUpstreamIsLoopback()) {
    const fromGit = await detectGitRepoRootForWorkspace();
    if (fromGit) root = fromGit.trim();
  }
  if (!root) return { patched: false };
  try {
    const res = await fetch(`${UPSTREAM}/api/company/tasks/${params.taskId}/context`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ workspace_attachment_paths: [root] }),
    });
    return { patched: res.ok, root };
  } catch {
    return { patched: false };
  }
}

function pickWorkerFinalizeSummary(input: {
  runSummaryInitial?: string;
  storedSummary: string;
  logTail: string;
  taskRunStatus: string;
  taskToolCalls: number;
}): string {
  const capSummary = (text: string): string => {
    const maxCharsRaw = Number(process.env.HSM_AGENT_CHAT_SUMMARY_MAX_CHARS ?? "200000");
    const maxChars = Number.isFinite(maxCharsRaw) ? maxCharsRaw : 200000;
    if (maxChars <= 0) return text;
    if (text.length <= maxChars) return text;
    return text.slice(-maxChars);
  };
  const runSummaryInitial = (input.runSummaryInitial ?? "").trim();
  const trimmedStored = input.storedSummary.trim();
  const trimmedLog = input.logTail.trim();
  const looksLikeInitialDispatchSummary =
    trimmedStored.length > 0 &&
    (runSummaryInitial.length > 0
      ? trimmedStored === runSummaryInitial ||
        trimmedStored.startsWith("Operator turn via agent loop") ||
        trimmedStored.startsWith("Skill dispatched to worker")
      : trimmedStored.startsWith("Operator turn via agent loop") ||
        trimmedStored.startsWith("Skill dispatched to worker"));
  if (trimmedLog.length > 0 && (looksLikeInitialDispatchSummary || !trimmedStored)) {
    return capSummary(trimmedLog);
  }
  if (trimmedStored.length > 0 && !looksLikeInitialDispatchSummary) {
    return capSummary(trimmedStored);
  }
  if (trimmedLog.length > 0) {
    return capSummary(trimmedLog);
  }
  return `Task runtime ${input.taskRunStatus} (${input.taskToolCalls} tool calls)`;
}

const HIGH_VALUE_WORKER_TOOLS = new Set([
  "bash",
  "read",
  "read_file",
  "grep",
  "edit",
  "write",
  "find",
  "search_files",
  "list_directory",
]);

function extractHighValueToolSignalsFromSummary(summary: string): Set<string> {
  const out = new Set<string>();
  const s = summary.toLowerCase();
  const loopMatch = s.match(/agentic tool loop:\s*\d+\s*tool calls\s*\(([^)]+)\)/i);
  if (loopMatch?.[1]) {
    for (const raw of loopMatch[1].split(",")) {
      const t = raw.trim().toLowerCase();
      if (HIGH_VALUE_WORKER_TOOLS.has(t)) out.add(t);
    }
  }
  const toolLabelRegex = /\btool(?:s)?:\s*([a-z0-9_,\-\s]+)/gi;
  for (const m of s.matchAll(toolLabelRegex)) {
    const group = m[1] ?? "";
    for (const raw of group.split(",")) {
      const t = raw.trim().toLowerCase();
      if (HIGH_VALUE_WORKER_TOOLS.has(t)) out.add(t);
    }
  }
  return out;
}

function extractHighValueToolSignalsFromRunMeta(meta: Record<string, unknown> | null | undefined): Set<string> {
  const out = new Set<string>();
  if (!meta) return out;
  const artifacts = toObject(meta.run_artifacts);
  const touched = Array.isArray(artifacts?.touched_files) ? artifacts?.touched_files : [];
  for (const row of touched) {
    const rec = toObject(row);
    const tools = Array.isArray(rec?.tools) ? rec?.tools : [];
    for (const tool of tools) {
      if (typeof tool !== "string") continue;
      const t = tool.trim().toLowerCase();
      if (HIGH_VALUE_WORKER_TOOLS.has(t)) out.add(t);
    }
  }
  return out;
}

function detectWorkerExecutionFailureReason(summary: string): string | null {
  const s = summary.toLowerCase();
  if (s.includes("llm unavailable for agentic execution")) return "Worker LLM unavailable for agentic execution.";
  if (s.includes("worker llm error")) return "Worker LLM error during execution.";
  if (s.includes("ollama returned status 404")) return "Ollama model/provider not available (404).";
  if (s.includes("no llm providers configured")) return "No LLM provider configured for worker runtime.";
  if (s.includes("no tool calls were executed")) return "Worker exited without executing any real tool calls.";
  if (s.includes("no successful non-dispatch tool completions observed")) return "Worker ran tools but none completed successfully.";
  if (s.includes("ended without a final answer (max turns reached)")) return "Worker exhausted tool loop without producing a final answer.";
  return null;
}

function sanitizeWorkerMirrorText(summary: string): string | null {
  let text = summary.replace(/\r\n/g, "\n").trim();
  const lower = text.toLowerCase();
  const replyIdx = lower.lastIndexOf("worker reply:");
  const successIdx = lower.lastIndexOf("worker success:");
  const startIdx = Math.max(replyIdx, successIdx);
  if (startIdx >= 0) {
    const marker = lower.startsWith("worker reply:", startIdx) ? "worker reply:" : "worker success:";
    text = text.slice(startIdx + marker.length).trim();
  }
  text = text.replace(/\n{2,}---\n\*agentic tool loop:[\s\S]*$/i, "").trim();
  text = text
    .split("\n")
    .filter((line) => !/^\s*worker (start|error|success|reply):/i.test(line))
    .join("\n")
    .trim();
  if (/^(we need to|i need to|need to|probably|unclear|hard to guess|maybe\b|might be\b)/i.test(text)) {
    const anchors = [
      /\bHere(?:'s| is)\b/i,
      /\bIt looks like\b/i,
      /\bIn\s+`?Cargo\.toml`?\b/i,
      /\bThe\s+(?:package|project|repository)\b/i,
    ];
    let best = -1;
    for (const re of anchors) {
      const m = re.exec(text);
      if (!m) continue;
      const idx = m.index ?? -1;
      if (idx <= 0) continue;
      if (best < 0 || idx < best) best = idx;
    }
    if (best > 0) text = text.slice(best).trim();
    if (best < 0) return null;
  }
  if (!text) return null;
  const low = text.toLowerCase();
  if (
    low.startsWith("routed this turn through the worker agent loop") ||
    low.startsWith("routed to worker — no conversational") ||
    low.startsWith("routed to worker (quick read/edit")
  ) {
    return null;
  }
  if (low.includes("agentic tool loop ended without a final answer")) return null;
  if (low.includes("worker tool execution required but no tool calls were executed")) return null;
  if (low.startsWith("worker error:")) return null;
  return text.slice(0, 12000);
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
  requireWorkerEvidence?: boolean;
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
    requireWorkerEvidence = false,
    runSummary,
    extraMeta,
    dispatchNoteText,
  } = params;

  const { agentRegistryId } = await resolveAgentForPersona(companyId, persona);
  await ensureTaskWorkspaceFromCompanyDefaults({ companyId, taskId }).catch(() => {});

  let activeTaskId = taskId;
  let checkout = await checkoutTaskForWorker(activeTaskId, persona);
  let checkoutFallbackUsed = false;
  const canFallbackTask =
    externalSystem === "worker-dispatch-chat" || externalSystem === "worker-dispatch";
  if (!checkout.ok && canFallbackTask) {
    const msg = String(checkout.error ?? "").toLowerCase();
    const shouldRetryWithFreshTask =
      msg.includes("task not found") ||
      msg.includes("already checked out") ||
      msg.includes("checkout failed");
    if (shouldRetryWithFreshTask) {
      const fallbackTaskId = await createFallbackDispatchTask({
        companyId,
        persona,
        skillSlug,
        sourceTaskId: taskId,
        runSummary,
      });
      if (fallbackTaskId) {
        activeTaskId = fallbackTaskId;
        checkout = await checkoutTaskForWorker(activeTaskId, persona);
        checkoutFallbackUsed = true;
      }
    }
  }

  if (!checkout.ok) {
    return {
      ok: false,
      error: checkout.error ?? "checkout failed",
      httpStatus: checkout.status || 502,
    };
  }

  const runId = await createAgentRun(companyId, agentRegistryId, activeTaskId, skillSlug, {
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
      body: JSON.stringify({
        meta: {
          skill: skillSlug,
          triggered_by: externalSystem,
          execution_mode: "pending",
          checkout_fallback_task: checkoutFallbackUsed ? activeTaskId : null,
          checkout_source_task: checkoutFallbackUsed ? taskId : null,
          ...extraMeta,
        },
      }),
    }).catch(() => {});
  }

  const recursiveScaffold = await ensureRecursiveDocsForRun({
    companyId,
    runId,
    persona,
    skillSlug,
    taskId: activeTaskId,
    userPrompt:
      (typeof extraMeta?.operator_message === "string" ? extraMeta.operator_message : runSummary) ??
      "",
  });
  if (recursiveScaffold) {
    let currentMeta: Record<string, unknown> = {};
    try {
      const runRes = await fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${runId}`);
      const runJson = (await runRes.json().catch(() => ({}))) as { run?: { meta?: Record<string, unknown> } };
      currentMeta = toObject(runJson.run?.meta) ?? {};
    } catch {
      currentMeta = {};
    }
    await fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${runId}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        meta: {
          ...currentMeta,
          recursive_run_path: recursiveScaffold.recursiveRunPath,
          recursive_requirements_path: recursiveScaffold.recursiveReqPath,
        },
      }),
    }).catch(() => {});
  }

  const shouldPersistDispatchNote =
    persistAgentNote && externalSystem !== "worker-dispatch-chat" && externalSystem !== "worker-dispatch";
  if (shouldPersistDispatchNote) {
    await fetch(`${UPSTREAM}/api/company/tasks/${activeTaskId}/stigmergic-note`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        text:
          dispatchNoteText ??
          `Dispatched skill \`${skillSlug}\` to worker runtime.${checkoutFallbackUsed ? ` (fallback task ${activeTaskId})` : ""}`,
        // Use "system" so the UI poll does not treat this dispatch line as the persona's reply.
        actor: "system",
      }),
    }).catch(() => {});
  }
  // Integrate existing coordinator path: allow spawn-rules to fan out background subtasks.
  await fetch(`${UPSTREAM}/api/company/companies/${companyId}/tasks/${activeTaskId}/spawn-subagents`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ actor: persona, reason: "operator-chat-dispatch" }),
  }).catch(() => {});

  const opMsg =
    (typeof extraMeta?.operator_message === "string" && extraMeta.operator_message.trim().length > 0
      ? extraMeta.operator_message.trim()
      : runSummary?.trim()) ??
    `Run skill ${skillSlug} for task ${activeTaskId}`;
  const shouldPrimeWorkspaceTools =
    (externalSystem === "worker-dispatch-chat" || externalSystem === "worker-dispatch") &&
    (looksLikeImplicitWorkspacePointer(opMsg) || looksLikeCodingToolIntent(opMsg));
  const executePrompt = shouldPrimeWorkspaceTools
    ? [
        opMsg,
        "",
        "Tool-first execution:",
        "- Inspect the attached workspace with real tools before answering.",
        "- For file/location requests, prefer glob/list_directory first, then read/grep as needed.",
        "- Return concrete file paths and a short evidence-based summary; do not answer from assumptions.",
      ].join("\n")
    : opMsg;
  const executeRes = await fetch(`${UPSTREAM}/api/company/tasks/${activeTaskId}/execute-worker`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      actor: persona,
      skill_slug: skillSlug,
      prompt: executePrompt,
    }),
  });
  if (!executeRes.ok) {
    const errText = await executeRes.text().catch(() => executeRes.statusText);
    return {
      ok: false,
      error: `worker execute start failed: ${errText || executeRes.statusText}`,
      httpStatus: executeRes.status || 502,
      runId,
    };
  }

  const endAt = Date.now() + Math.max(0, waitForTelemetryMs);
  let latestSummary: string | null = null;
  let latestMode: "pending" | "worker" | "llm_simulated" = "pending";
  let sawWorkerEvidence = false;
  let sawHighValueToolEvidence = false;
  const dispatchStartedAt = Date.now();
  let baselineActivityMs = 0;
  try {
    const activityRes = await fetch(`${UPSTREAM}/api/company/runtime/activity`);
    if (activityRes.ok) {
      const activityJson = (await activityRes.json().catch(() => ({}))) as {
        activity?: { last_activity_ms?: number };
      };
      baselineActivityMs =
        typeof activityJson.activity?.last_activity_ms === "number"
          ? activityJson.activity.last_activity_ms
          : 0;
    }
  } catch {
    baselineActivityMs = 0;
  }

  while (Date.now() < endAt) {
    const [runRes, tasksRes, activityRes] = await Promise.all([
      fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${runId}`),
      fetch(`${UPSTREAM}/api/company/companies/${companyId}/tasks`),
      fetch(`${UPSTREAM}/api/company/runtime/activity`).catch(() => null),
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
    const task = (tasksJson.tasks ?? []).find((t) => t.id === activeTaskId);
    const taskRunStatus = (task?.run?.status ?? "").toLowerCase();
    const taskToolCalls = task?.run?.tool_calls ?? 0;
    // Runtime snapshot tool_calls already reflects real tool completions (dispatch is excluded).
    // Treat any successful tool completion as execution evidence.
    const observedFromTask = taskToolCalls > 0;
    let observedFromRuntime = false;
    let runtimeToolName = "";
    if (activityRes && "ok" in activityRes && activityRes.ok) {
      const activityJson = (await activityRes.json().catch(() => ({}))) as {
        activity?: {
          last_activity_ms?: number;
          phase?: string;
          tool_name?: string | null;
        };
      };
      const lastActivityMs =
        typeof activityJson.activity?.last_activity_ms === "number"
          ? activityJson.activity.last_activity_ms
          : 0;
      const activityAdvanced =
        lastActivityMs > Math.max(baselineActivityMs, dispatchStartedAt - 1_000);
      const toolName =
        typeof activityJson.activity?.tool_name === "string" ? activityJson.activity.tool_name.trim().toLowerCase() : "";
      runtimeToolName = toolName;
      const hasNonDispatchToolName =
        toolName.length > 0 && toolName !== "worker_dispatch" && toolName !== "claude_harness";
      const phase = typeof activityJson.activity?.phase === "string" ? activityJson.activity.phase : "";
      observedFromRuntime =
        activityAdvanced &&
        (hasNonDispatchToolName ||
          ((phase === "start" || phase === "finish") && hasNonDispatchToolName));
    }
    const observedWorker = observedFromTask || observedFromRuntime;
    sawWorkerEvidence = sawWorkerEvidence || observedWorker;
    const highValueFromRuntime =
      runtimeToolName.length > 0 &&
      runtimeToolName !== "worker_dispatch" &&
      runtimeToolName !== "claude_harness" &&
      HIGH_VALUE_WORKER_TOOLS.has(runtimeToolName);
    const highValueFromSummary = extractHighValueToolSignalsFromSummary(
      [runJson.run?.summary ?? "", task?.run?.log_tail ?? "", runSummary ?? ""].join("\n"),
    );
    const highValueFromMeta = extractHighValueToolSignalsFromRunMeta(runJson.run?.meta ?? null);
    sawHighValueToolEvidence =
      sawHighValueToolEvidence ||
      highValueFromRuntime ||
      highValueFromSummary.size > 0 ||
      highValueFromMeta.size > 0;

    latestMode = sawWorkerEvidence ? "worker" : latestMode;
    if (observedWorker && runJson.run?.meta?.execution_mode !== "worker") {
      await patchRunMetaExecutionMode(companyId, runId, runJson.run?.meta, "worker", true);
    }

    if (taskRunStatus === "success" || taskRunStatus === "error") {
      if (requireWorkerEvidence && (!sawWorkerEvidence || !sawHighValueToolEvidence)) {
        const summary = "Worker run finished without tool evidence; refusing optimistic completion.";
        await fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${runId}`, {
          method: "PATCH",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            status: "error",
            summary,
            finished_at: true,
            meta: {
              ...(runJson.run?.meta ?? {}),
              execution_mode: "pending",
              execution_verified: false,
              needs_human: true,
            },
          }),
        }).catch(() => {});
        return {
          ok: false,
          error: !sawWorkerEvidence
            ? summary
            : "Worker run finished without high-value tool evidence; need at least one successful bash/read/grep/edit/write/find/search/list_directory signal.",
          httpStatus: 409,
          runId,
        };
      }
      const finalMode = sawWorkerEvidence ? "worker" : "llm_simulated";
      const logTail =
        typeof task?.run?.log_tail === "string" && task.run.log_tail.trim() ? task.run.log_tail : "";
      latestSummary = pickWorkerFinalizeSummary({
        runSummaryInitial: runSummary,
        storedSummary: (runJson.run?.summary ?? "").trim(),
        logTail,
        taskRunStatus,
        taskToolCalls,
      });
      const hardFailureReason = detectWorkerExecutionFailureReason(latestSummary);
      if ((taskRunStatus === "success" && (!sawWorkerEvidence || !sawHighValueToolEvidence)) || hardFailureReason) {
        const summary = hardFailureReason
          ? `${hardFailureReason} No non-dispatch tool execution observed.`
          : !sawWorkerEvidence
            ? "Worker run finished without non-dispatch tool evidence; refusing optimistic completion."
            : "Worker run finished without high-value tool evidence; refusing optimistic completion.";
        await fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${runId}`, {
          method: "PATCH",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            status: "error",
            summary,
            finished_at: true,
            meta: {
              ...(runJson.run?.meta ?? {}),
              execution_mode: "pending",
              execution_verified: false,
              needs_human: true,
            },
          }),
        }).catch(() => {});
        return {
          ok: false,
          error: summary,
          httpStatus: 409,
          runId,
        };
      }
      await fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${runId}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          status: taskRunStatus,
          summary: latestSummary,
          finished_at: true,
          meta: {
            ...(runJson.run?.meta ?? {}),
            execution_mode: finalMode,
            execution_verified: sawWorkerEvidence && sawHighValueToolEvidence,
          },
        }),
      });
      // Mirror the worker reply back into the task thread as a stigmergic note.
      if (
        persistAgentNote &&
        taskRunStatus === "success" &&
        sawWorkerEvidence &&
        (externalSystem === "worker-dispatch-chat" || externalSystem === "worker-dispatch") &&
        typeof latestSummary === "string"
      ) {
        const mirrorText = sanitizeWorkerMirrorText(latestSummary);
        if (mirrorText) {
          await fetch(`${UPSTREAM}/api/company/tasks/${activeTaskId}/stigmergic-note`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ text: mirrorText, actor: persona }),
          }).catch(() => {});
        }
      }
      return {
        ok: true,
        runId,
        status: taskRunStatus as "success" | "error",
        executionMode: finalMode,
        workerEvidence: sawWorkerEvidence,
        executionVerified: sawWorkerEvidence && sawHighValueToolEvidence,
        summary: latestSummary,
        finalized: true,
      };
    }

    await new Promise((r) => setTimeout(r, 2500));
  }

  // Soft-timeout behavior: keep the run alive if no evidence was observed yet.
  // We still enforce strict proof if/when the task reaches success/error without evidence.
  if (requireWorkerEvidence && (!sawWorkerEvidence || !sawHighValueToolEvidence)) {
    const pendingSummary =
      latestSummary ??
      "No worker tool activity observed within telemetry window yet; run remains pending until evidence arrives.";
    await fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${runId}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        status: "running",
        summary: pendingSummary,
        meta: {
          execution_mode: latestMode,
          execution_verified: false,
          evidence_pending: true,
          high_value_evidence_pending: !sawHighValueToolEvidence,
          evidence_timeout_ms: waitForTelemetryMs,
        },
      }),
    }).catch(() => {});
    return {
      ok: true,
      runId,
      status: "running",
      executionMode: latestMode,
      workerEvidence: sawWorkerEvidence,
      executionVerified: false,
      summary: pendingSummary,
      finalized: false,
    };
  }

  return {
    ok: true,
    runId,
    status: "running",
    executionMode: latestMode,
    workerEvidence: sawWorkerEvidence,
    executionVerified: sawWorkerEvidence && sawHighValueToolEvidence,
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

  const reply = extractReplyFromChatCompletionPayload(data).trim();
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

// ---------------------------------------------------------------------------
// Context compaction — keeps LLM message history bounded so long operator
// threads don't blow the context window or inflate costs.
//
// Strategy:
//   • If notes ≤ COMPACT_NOTE_THRESHOLD AND total chars ≤ COMPACT_CHAR_THRESHOLD → no-op.
//   • Otherwise: take the older (N - COMPACT_KEEP_RECENT) notes, build a
//     compact prose summary, fold it into the system prompt extension, and
//     only send the COMPACT_KEEP_RECENT most recent notes as actual
//     user/assistant turns.
//   • The summary is also written to company_memory_entries (scope=shared,
//     source=agent_chat_compaction) so the supermemory can surface it later
//     as relevant history.
// ---------------------------------------------------------------------------

/** Compact when the thread exceeds this many notes … */
const COMPACT_NOTE_THRESHOLD = 18;
/** … or this many total characters across all note texts. */
const COMPACT_CHAR_THRESHOLD = 7_000;
/** How many recent notes to keep verbatim as LLM messages after compaction. */
const COMPACT_KEEP_RECENT = 8;

export type CompactionResult = {
  /** Whether compaction was applied. */
  compacted: boolean;
  /** Older notes condensed into prose (present only when compacted). */
  compactionSummary: string | null;
  /** Number of notes that were compacted away. */
  compactedCount: number;
  /** LLM message history ready to be sent (recent notes only after compaction). */
  messageHistory: Array<{ role: "user" | "assistant"; content: string }>;
};

/**
 * Decide whether to compact and build the message history + optional summary.
 *
 * Call this **before** building the `messages` array for the LLM.  When
 * `compacted === true`, prepend `compactionSummary` to the system prompt.
 */
export function compactNotesForLlm(notes: StigNote[]): CompactionResult {
  const filtered = notes.filter((n) => n.text?.trim());
  const totalChars = filtered.reduce((s, n) => s + n.text.length, 0);
  const needsCompaction =
    filtered.length > COMPACT_NOTE_THRESHOLD || totalChars > COMPACT_CHAR_THRESHOLD;

  if (!needsCompaction) {
    return {
      compacted: false,
      compactionSummary: null,
      compactedCount: 0,
      messageHistory: filtered.map((n) => ({
        role: (n.actor === "operator" ? "user" : "assistant") as "user" | "assistant",
        content: n.text,
      })),
    };
  }

  const olderNotes = filtered.slice(0, -COMPACT_KEEP_RECENT);
  const recentNotes = filtered.slice(-COMPACT_KEEP_RECENT);

  // Build a compact prose summary of the older notes.
  const summaryLines = olderNotes.map((n) => {
    const actor = n.actor === "operator" ? "Operator" : n.actor;
    const ts = n.at ? n.at.slice(0, 16).replace("T", " ") : "";
    const snippet = n.text.trim().replace(/\n+/g, " ").slice(0, 240);
    const ellipsis = n.text.length > 240 ? "…" : "";
    return `- [${actor}${ts ? ` @ ${ts}` : ""}]: ${snippet}${ellipsis}`;
  });

  const compactionSummary = [
    `## Compacted conversation history (${olderNotes.length} earlier message${olderNotes.length === 1 ? "" : "s"})`,
    "",
    "The following is a condensed record of the thread before the current exchange.",
    "Treat it as authoritative context but do not repeat or re-summarize it in your reply.",
    "",
    summaryLines.join("\n"),
  ].join("\n");

  return {
    compacted: true,
    compactionSummary,
    compactedCount: olderNotes.length,
    messageHistory: recentNotes.map((n) => ({
      role: (n.actor === "operator" ? "user" : "assistant") as "user" | "assistant",
      content: n.text,
    })),
  };
}

/**
 * Persist a compaction summary to company shared memory (supermemory) so it
 * remains searchable after the active task thread is long gone.
 *
 * Fire-and-forget — callers should `.catch(() => {})` the returned promise.
 */
export async function saveCompactionToMemory(
  companyId: string,
  taskId: string,
  persona: string,
  summary: string,
): Promise<void> {
  await fetch(`${UPSTREAM}/api/company/companies/${companyId}/memory`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      title: `Chat compaction · ${persona} · task ${taskId.slice(0, 8)}`,
      body: summary,
      scope: "shared",
      source: "agent_chat_compaction",
      kind: "general",
      tags: ["compaction", `task:${taskId}`, `persona:${persona}`],
    }),
  });
}
