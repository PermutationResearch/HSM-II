/**
 * Server-side NDJSON stream for operator chat: proxies Company OS runtime SSE (tool events)
 * while worker dispatch runs, and emits Claude Code–compatible `stream_event` lines
 * (Anthropic Messages API shapes: message_start, content_block_delta, message_stop) on the
 * OpenRouter streaming path. Legacy `{ type: "delta" }` is omitted there to avoid double text;
 * the client consumes `stream_event` first and still accepts `delta` if present.
 *
 * When dispatch still goes to the worker (skills / execution / no key), an optional **companion**
 * OpenRouter stream runs in parallel (serialized with runtime SSE) so the rail shows live
 * `stream_event` text while tools run. Disable with `HSM_DISABLE_WORKER_COMPANION_STREAM=1`.
 */

import {
  buildCompactedContextBundle,
  buildStrictToolFlowTrace,
  buildSystemPrompt,
  CHAT_MODEL,
  deriveToolExecutionPolicy,
  detectSkillDispatch,
  dispatchSkillToWorker,
  finalizeAgentRun,
  createAgentRun,
  looksLikeExecutionIntent,
  OR_BASE,
  parseOptimizeCommand,
  readOpenRouterApiKey,
  resolveAgentForPersona,
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
import { asObject } from "@/app/lib/runtime-contract";

export type AgentChatRequestBody = {
  taskId: string;
  persona: string;
  companyId?: string;
  title?: string;
  role?: string;
  notes: Array<{ at: string; actor: string; text: string }>;
};

export type NdjsonLineWriter = (obj: Record<string, unknown>) => Promise<void>;

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
  if (kind === "skill" && skillSlug) {
    return `You are ${persona} in the Company OS operator chat. The operator invoked skill \`${skillSlug}\`; a separate worker runtime is executing it (tools, checkout, telemetry). Stream a concise reply in character: acknowledge the skill, describe what this class of work usually involves at a high level, and say that concrete tool traces appear as separate live events in the UI. Do not invent tool outputs, file contents, or command results you have not been shown.`;
  }
  return `You are ${persona} in the Company OS operator chat. The operator message is being handled by a leased worker (tools, task checkout). Stream a concise reply: acknowledge the request, outline your reasoning and next checks at a high level, and state that tool/runtime events stream separately—never fabricate shell output, paths, or file contents.`;
}

/** Read Company OS runtime SSE and emit each `data:` JSON payload as `{ type: "runtime", payload }`. */
export async function forwardRuntimeSseAsNdjson(
  upstreamBase: string,
  signal: AbortSignal,
  write: NdjsonLineWriter,
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
          const et = typeof payload.event_type === "string" ? payload.event_type : "";
          const inner = payload.stream_event;
          if (et === "stream_event" && inner != null && typeof inner === "object") {
            await write(wrapSdkStreamEvent(inner as Record<string, unknown>) as Record<string, unknown>);
          } else {
            await write({ type: "runtime", payload });
          }
        } catch {
          await write({ type: "runtime_raw", text: data });
        }
      }
    }
  }
}

async function streamOpenRouterChat(params: {
  apiKey: string;
  messages: Array<{ role: "system" | "user" | "assistant"; content: string }>;
  maxTokens: number;
  temperature: number;
  write: NdjsonLineWriter;
}): Promise<string> {
  const { apiKey, messages, maxTokens, temperature, write } = params;
  const res = await fetch(`${OR_BASE}/chat/completions`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${apiKey}`,
      "Content-Type": "application/json",
      "HTTP-Referer": "https://hsm.ai",
      "X-Title": "HSM Company Console",
    },
    body: JSON.stringify({
      model: CHAT_MODEL,
      messages,
      max_tokens: maxTokens,
      temperature,
      stream: true,
    }),
  });

  if (!res.ok || !res.body) {
    const errText = await res.text().catch(() => res.statusText);
    throw new Error(`LLM ${res.status}: ${errText}`);
  }

  await write(wrapSdkStreamEvent(anthropicMessageStart(CHAT_MODEL)) as Record<string, unknown>);
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
      if (!t.startsWith("data:")) continue;
      const data = t.slice(5).trim();
      if (data === "[DONE]") continue;
      try {
        const j = JSON.parse(data) as Record<string, unknown>;
        const choices = Array.isArray(j.choices) ? j.choices : [];
        const ch0 = asObject(choices[0]);
        const delta = asObject(ch0?.delta);
        const piece = typeof delta?.content === "string" ? delta.content : "";
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
  if (tail.startsWith("data:")) {
    const data = tail.slice(5).trim();
    if (data && data !== "[DONE]") {
      try {
        const j = JSON.parse(data) as Record<string, unknown>;
        const choices = Array.isArray(j.choices) ? j.choices : [];
        const ch0 = asObject(choices[0]);
        const delta = asObject(ch0?.delta);
        const piece = typeof delta?.content === "string" ? delta.content : "";
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

async function withRuntimeMirror(
  upstream: string,
  write: NdjsonLineWriter,
  work: () => Promise<void>,
): Promise<void> {
  const ac = new AbortController();
  const mirror = forwardRuntimeSseAsNdjson(upstream, ac.signal, write);
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
  const lastOperatorText = [...notes].reverse().find((n) => n.actor === "operator")?.text ?? "";

  let agentRegistryId: string | undefined;
  let agentAdapterConfig: Record<string, unknown> | null = null;
  let detectedSkill: string | null = null;

  if (companyId) {
    const { agentRegistryId: aid, allKnownSlugs, agentAdapterConfig: cfg } = await resolveAgentForPersona(
      companyId,
      persona,
    );
    agentRegistryId = aid;
    agentAdapterConfig = cfg;
    detectedSkill = detectSkillDispatch(notes, allKnownSlugs);
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
    const companionKey = companionOptOut ? undefined : readOpenRouterApiKey();
    await write({
      type: "phase",
      phase: "skill_dispatch_runtime",
      live_companion_stream: !!companionKey,
    });
    const ser = createSerializedNdjsonWriter(write);
    let result: WorkerDispatchResult | undefined;
    await withRuntimeMirror(UPSTREAM, ser, async () => {
      const dispatchPromise = dispatchSkillToWorker({
        companyId,
        taskId,
        persona,
        skillSlug: detectedSkill,
        externalSystem: "worker-dispatch",
        persistAgentNote: true,
        runSummary: `Skill turn via agent loop runtime: ${detectedSkill}`,
        dispatchNoteText: `Running \`${detectedSkill}\` in worker agent loop runtime.`,
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
      if (companionKey) {
        const companion = streamOpenRouterChat({
          apiKey: companionKey,
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
    });
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
    await write({
      type: "done",
      ok: true,
      reply: `Running \`${detectedSkill}\` in worker agent loop runtime.`,
      at: new Date().toISOString(),
      run_id: result.runId,
      skill: detectedSkill,
      status: result.status,
      execution_mode: result.executionMode,
      finalized: result.finalized,
    });
    return;
  }

  if (!detectedSkill && companyId) {
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
    const openRouterKey = readOpenRouterApiKey();
    const executionIntent = looksLikeExecutionIntent(lastOperatorText);
    const routeWorker = executionIntent || !openRouterKey;
    if (routeWorker) {
      const companionOptOut = process.env.HSM_DISABLE_WORKER_COMPANION_STREAM === "1";
      const sidecarKey = companionOptOut ? undefined : openRouterKey;
      await write({
        type: "phase",
        phase: "operator_chat_worker",
        execution_intent: executionIntent,
        openrouter_configured: !!openRouterKey,
        token_stream: !!sidecarKey,
        companion_sidecar: !!sidecarKey,
        reason: executionIntent
          ? "execution_intent_uses_worker"
          : "missing_openrouter_key_in_next_env",
      });
      const policy = deriveToolExecutionPolicy(agentAdapterConfig);
      const compact = await buildCompactedContextBundle({
        companyId,
        taskId,
        agentRegistryId,
        budgetBytes: 5200,
      });
      const dispatchNoteText =
        !executionIntent && !openRouterKey
          ? "Routed to worker — no OpenRouter key in this Next.js process, so this panel cannot stream answer tokens. Set OPENROUTER_API_KEY (or HSM_OPENROUTER_API_KEY) for web/company-console and restart, or put it in the repo-root .env (loaded by next.config). Worker is still running in the background."
          : "Routed this turn through the worker agent loop runtime.";
      const ser = createSerializedNdjsonWriter(write);
      let result: WorkerDispatchResult | undefined;
      await withRuntimeMirror(UPSTREAM, ser, async () => {
        const dispatchPromise = dispatchSkillToWorker({
          companyId,
          taskId,
          persona,
          skillSlug: "operator-chat",
          externalSystem: "worker-dispatch-chat",
          persistAgentNote: true,
          runSummary: `Operator turn via agent loop runtime: ${lastOperatorText.slice(0, 120)}`,
          dispatchNoteText,
          extraMeta: {
            trigger: "operator_chat",
            operator_message: lastOperatorText.slice(0, 1000),
            intent: executionIntent ? "execution" : "analysis",
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
        if (sidecarKey) {
          const companion = streamOpenRouterChat({
            apiKey: sidecarKey,
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
      });
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
      await write({
        type: "done",
        ok: true,
        at: new Date().toISOString(),
        run_id: result.runId,
        skill: "worker-agent-loop",
        status: result.status,
        execution_mode: result.executionMode,
        finalized: result.finalized,
        streaming: true,
      });
      return;
    }
    await write({ type: "phase", phase: "openrouter_llm", chat_mode: "conversational_company" });
  }

  const key = readOpenRouterApiKey();
  if (!key) {
    await write({
      type: "error",
      message:
        "OpenRouter API key not configured for Next.js (set OPENROUTER_API_KEY or HSM_OPENROUTER_API_KEY in web/company-console/.env.local or repo-root .env).",
    });
    return;
  }

  await write({ type: "phase", phase: "openrouter_llm" });
  const system = await Promise.race([
    buildSystemPrompt(persona, companyId, detectedSkill, taskId, "operator_chat"),
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
    const compact = [
      compactDigest ? `## Operator Thread Digest\n${compactDigest.slice(0, 1800)}` : "",
      addon ? `## Task LLM Context (Compacted)\n${addon.slice(0, 2800)}` : "",
    ]
      .filter(Boolean)
      .join("\n\n");
    if (compact) {
      enrichedSystem = `${system}\n\n${compact}`;
    }
  }

  const messages = notes
    .filter((n) => n.text?.trim())
    .map((n) => ({
      role: n.actor === "operator" ? ("user" as const) : ("assistant" as const),
      content: n.text,
    }));

  if (messages.length === 0 || messages[messages.length - 1].role !== "user") {
    await write({ type: "error", message: "No operator message to respond to" });
    return;
  }

  let reply: string;
  try {
    reply = await streamOpenRouterChat({
      apiKey: key,
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
