/**
 * POST /api/agent-chat-reply
 *
 * Fetches the full agent context from the Company OS (briefing, VISION.md,
 * skills, teammates, memory) and calls OpenRouter with a rich system prompt.
 *
 * Detects `run [skill]`, `run skill`, etc., creates an agent-run, executes via LLM,
 * and returns run_id + skill for live status in the workspace rail.
 */
import { NextRequest, NextResponse } from "next/server";

import {
  buildStrictToolFlowTrace,
  buildCompactedContextBundle,
  buildSystemPrompt,
  CHAT_MODEL,
  compactNotesForLlm,
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
  saveCompactionToMemory,
  UPSTREAM,
  upsertThreadSessionState,
} from "@/app/lib/agent-chat-server";
import { asObject } from "@/app/lib/runtime-contract";

type StigNote = { at: string; actor: string; text: string };

interface RequestBody {
  taskId: string;
  persona: string;
  companyId?: string;
  title?: string;
  role?: string;
  notes: StigNote[];
}

export async function POST(req: NextRequest) {
  const bodyRaw = await req.json().catch(() => null);
  const bodyObj = asObject(bodyRaw);
  const body: RequestBody | null = bodyObj
    ? {
        taskId: typeof bodyObj.taskId === "string" ? bodyObj.taskId : "",
        persona: typeof bodyObj.persona === "string" ? bodyObj.persona : "",
        companyId: typeof bodyObj.companyId === "string" ? bodyObj.companyId : undefined,
        title: typeof bodyObj.title === "string" ? bodyObj.title : undefined,
        role: typeof bodyObj.role === "string" ? bodyObj.role : undefined,
        notes: Array.isArray(bodyObj.notes) ? (bodyObj.notes as StigNote[]) : [],
      }
    : null;
  if (!body || !body.taskId || !body.persona) {
    return NextResponse.json({ error: "taskId and persona required" }, { status: 400 });
  }

  const { taskId, persona, companyId, notes } = body;
  const lastOperatorText = [...notes].reverse().find((n) => n.actor === "operator")?.text ?? "";

  let agentRegistryId: string | undefined;
  let agentAdapterConfig: Record<string, unknown> | null = null;
  let detectedSkill: string | null = null;

  if (companyId) {
    const { agentRegistryId: aid, allKnownSlugs, agentAdapterConfig: cfg } = await resolveAgentForPersona(companyId, persona);
    agentRegistryId = aid;
    agentAdapterConfig = cfg;
    detectedSkill = detectSkillDispatch(notes, allKnownSlugs);
  }

  let runId: string | null = null;
  const strictFlow = companyId
    ? await buildStrictToolFlowTrace(companyId, lastOperatorText)
    : null;
  if (detectedSkill && companyId) {
    const policy = deriveToolExecutionPolicy(agentAdapterConfig);
    const compact = await buildCompactedContextBundle({
      companyId,
      taskId,
      agentRegistryId,
      budgetBytes: 5200,
    });
    const result = await dispatchSkillToWorker({
      companyId,
      taskId,
      persona,
      skillSlug: detectedSkill,
      externalSystem: "worker-dispatch",
      persistAgentNote: true,
      waitForTelemetryMs: 120_000,
      requireWorkerEvidence: true,
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
    if (!result.ok) {
      return NextResponse.json({ error: result.error, run_id: result.runId ?? undefined }, { status: result.httpStatus });
    }
    runId = result.runId;
    await upsertThreadSessionState({
      companyId,
      persona,
      taskId,
      runId: runId ?? undefined,
      state: {
        mode: "skill",
        skill: detectedSkill,
        loop_state: result.status === "running" ? "running" : "completed",
        compact_bytes: compact.bytes,
      },
    });
    return NextResponse.json({
      ok: true,
      reply: result.finalized
        ? `Completed \`${detectedSkill}\` in worker agent runtime.`
        : `Dispatched \`${detectedSkill}\` to worker runtime. Waiting for completion evidence.`,
      at: new Date().toISOString(),
      run_id: result.runId ?? undefined,
      skill: detectedSkill,
      status: result.status,
      execution_mode: result.executionMode,
      worker_evidence: result.workerEvidence,
      execution_verified: result.executionVerified,
      finalized: result.finalized,
    });
  }

  // `optimize [task|plan|signature]` command path -> optimize APIs + tracked run.
  if (!detectedSkill && companyId) {
    const optimize = parseOptimizeCommand(lastOperatorText);
    if (optimize) {
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
        return NextResponse.json({ error: "Failed to create optimize run" }, { status: 502 });
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
        await finalizeAgentRun(
          companyId,
          optimizeRunId,
          `Optimize failed: ${optJson.error ?? optRes.statusText}`,
          "error",
        );
        return NextResponse.json({ error: optJson.error ?? "optimize failed" }, { status: 502 });
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
      const noteData = { context_notes: noteObj?.context_notes };
      return NextResponse.json({
        ok: true,
        reply: optimizeReply,
        at: new Date().toISOString(),
        run_id: optimizeRunId,
        skill: `optimize:${optimize.kind}`,
        context_notes: noteData.context_notes,
      });
    }
  }

  if (!detectedSkill && companyId) {
    const openRouterKey = readOpenRouterApiKey();
    const executionIntent = looksLikeExecutionIntent(lastOperatorText);
    const routeWorker = executionIntent || !openRouterKey;
    if (routeWorker) {
      const policy = deriveToolExecutionPolicy(agentAdapterConfig);
      const compact = await buildCompactedContextBundle({
        companyId,
        taskId,
        agentRegistryId,
        budgetBytes: 5200,
      });
      const dispatchNoteText =
        !executionIntent && !openRouterKey
          ? "Routed to worker — no OpenRouter key in this Next.js process (set OPENROUTER_API_KEY or HSM_OPENROUTER_API_KEY). Worker runs without streamed tokens in this UI until the loop finishes."
          : "Routed this turn through the worker agent loop runtime.";
      const result = await dispatchSkillToWorker({
        companyId,
        taskId,
        persona,
        skillSlug: "operator-chat",
        externalSystem: "worker-dispatch-chat",
        persistAgentNote: true,
        waitForTelemetryMs: 120_000,
        requireWorkerEvidence: true,
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
      if (!result.ok) {
        return NextResponse.json({ error: result.error, run_id: result.runId ?? undefined }, { status: result.httpStatus });
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
      return NextResponse.json({
        ok: true,
        at: new Date().toISOString(),
        run_id: result.runId ?? undefined,
        skill: "worker-agent-loop",
        status: result.status,
        execution_mode: result.executionMode,
        worker_evidence: result.workerEvidence,
        execution_verified: result.executionVerified,
        finalized: result.finalized,
        streaming: true,
      });
    }
  }

  const key = readOpenRouterApiKey();
  if (!key) {
    return NextResponse.json(
      { error: "OpenRouter API key not configured (OPENROUTER_API_KEY or HSM_OPENROUTER_API_KEY)" },
      { status: 500 },
    );
  }

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

  // Compact the note history so long threads don't overflow the context window.
  const compaction = compactNotesForLlm(notes);
  if (compaction.compacted && compaction.compactionSummary) {
    enrichedSystem = `${enrichedSystem}\n\n${compaction.compactionSummary}`;
    if (companyId) {
      saveCompactionToMemory(companyId, taskId, persona, compaction.compactionSummary).catch(() => {});
    }
  }

  const messages = compaction.messageHistory;

  if (messages.length === 0 || messages[messages.length - 1].role !== "user") {
    return NextResponse.json({ error: "No operator message to respond to" }, { status: 400 });
  }

  const llmRes = await fetch(`${OR_BASE}/chat/completions`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${key}`,
      "Content-Type": "application/json",
      "HTTP-Referer": "https://hsm.ai",
      "X-Title": "HSM Company Console",
    },
    body: JSON.stringify({
      model: CHAT_MODEL,
      messages: [{ role: "system", content: enrichedSystem }, ...messages],
      max_tokens: detectedSkill ? 1024 : 768,
      temperature: detectedSkill ? 0.4 : 0.7,
    }),
  });

  if (!llmRes.ok) {
    const errText = await llmRes.text().catch(() => llmRes.statusText);
    if (runId && companyId) {
      await finalizeAgentRun(companyId, runId, `LLM error: ${errText}`, "error");
    }
    return NextResponse.json({ error: `LLM ${llmRes.status}: ${errText}` }, { status: 502 });
  }

  const dataRaw = await llmRes.json().catch(() => ({}));
  const dataObj = asObject(dataRaw);
  const errorObj = asObject(dataObj?.error);
  const choices = Array.isArray(dataObj?.choices) ? dataObj.choices : [];
  const firstChoice = asObject(choices[0]);
  const firstMsg = asObject(firstChoice?.message);
  const data = {
    error: typeof errorObj?.message === "string" ? { message: errorObj.message } : undefined,
    content: typeof firstMsg?.content === "string" ? firstMsg.content : "",
  };

  if (data.error) {
    if (runId && companyId) {
      await finalizeAgentRun(companyId, runId, data.error.message ?? "LLM error", "error");
    }
    return NextResponse.json({ error: data.error.message ?? "LLM error" }, { status: 502 });
  }

  const reply = data.content.trim();
  if (!reply) {
    return NextResponse.json({ error: "Empty response from LLM" }, { status: 502 });
  }

  if (runId && companyId && detectedSkill) {
    await finalizeAgentRun(companyId, runId, reply.slice(0, 1000), "success");
  }

  const noteRes = await fetch(`${UPSTREAM}/api/company/tasks/${taskId}/stigmergic-note`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ text: reply, actor: persona }),
  });

  const noteRaw = await noteRes.json().catch(() => ({}));
  const noteObj = asObject(noteRaw);
  const noteData = {
    context_notes: noteObj?.context_notes,
    error: typeof noteObj?.error === "string" ? noteObj.error : undefined,
  };

  return NextResponse.json({
    ok: true,
    reply,
    at: new Date().toISOString(),
    run_id: runId ?? undefined,
    skill: detectedSkill ?? undefined,
    context_notes: noteData.context_notes,
  });
}
