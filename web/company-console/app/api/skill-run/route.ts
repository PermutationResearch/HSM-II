/**
 * POST /api/skill-run
 *
 * Cron- or script-triggered skill execution: validates a shared secret, creates an
 * agent-run (with optional idempotency via external_run_id), runs the same LLM skill
 * path as operator chat, and optionally writes a stigmergic note on the task.
 *
 * Auth: `Authorization: Bearer <HSM_SKILL_RUN_SECRET>` or JSON body `{ "secret": "..." }`.
 */
import { NextRequest, NextResponse } from "next/server";

import {
  buildStrictToolFlowTrace,
  buildCompactedContextBundle,
  deriveToolExecutionPolicy,
  dispatchSkillToWorker,
  resolveAgentForPersona,
  resolveSkillSlugHint,
  upsertThreadSessionState,
} from "@/app/lib/agent-chat-server";

interface Body {
  companyId?: string;
  taskId?: string;
  persona?: string;
  skill?: string;
  /** Idempotent re-run: same (company, external_system, external_run_id) returns existing run */
  external_run_id?: string;
  instruction?: string;
  secret?: string;
  /** Default true: append agent reply to task stigmergic notes */
  persist_note?: boolean;
  /** Optional blocking wait for worker telemetry/finalization (default 15000) */
  wait_ms?: number;
}

function authorized(req: NextRequest, bodySecret?: string): boolean {
  const expected = process.env.HSM_SKILL_RUN_SECRET?.trim();
  if (!expected) return false;
  const auth = req.headers.get("authorization")?.trim();
  if (auth === `Bearer ${expected}`) return true;
  if (bodySecret === expected) return true;
  return false;
}

export async function POST(req: NextRequest) {
  if (!process.env.HSM_SKILL_RUN_SECRET?.trim()) {
    return NextResponse.json(
      { error: "HSM_SKILL_RUN_SECRET is not configured on the server" },
      { status: 503 },
    );
  }

  const body = (await req.json().catch(() => null)) as Body | null;
  if (!body || !authorized(req, body.secret)) {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  const companyId = body.companyId?.trim();
  const taskId = body.taskId?.trim();
  const persona = body.persona?.trim();
  const skillHint = body.skill?.trim();

  if (!companyId || !taskId || !persona || !skillHint) {
    return NextResponse.json(
      { error: "companyId, taskId, persona, and skill are required" },
      { status: 400 },
    );
  }

  const { allKnownSlugs, agentRegistryId, agentAdapterConfig } = await resolveAgentForPersona(companyId, persona);
  const skillSlug = resolveSkillSlugHint(skillHint, allKnownSlugs);
  if (!skillSlug) {
    return NextResponse.json(
      { error: `Unknown skill for this agent: ${skillHint}`, known: allKnownSlugs },
      { status: 400 },
    );
  }

  const waitForTelemetryMs = Math.min(Math.max(body.wait_ms ?? 15_000, 0), 120_000);
  const policy = deriveToolExecutionPolicy(agentAdapterConfig);
  const strictFlow = await buildStrictToolFlowTrace(companyId, body.instruction?.trim() || `run ${skillSlug}`);
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
    skillSlug,
    externalSystem: "skill-run-api",
    externalRunId: body.external_run_id?.trim(),
    persistAgentNote: body.persist_note !== false,
    waitForTelemetryMs,
    requireWorkerEvidence: true,
    runSummary: `Skill run via worker: ${skillSlug}`,
    extraMeta: {
      trigger: "skill_run_api",
      operator_message: body.instruction?.slice(0, 1000) ?? "",
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
    return NextResponse.json(
      { error: result.error, run_id: result.runId ?? undefined },
      { status: result.httpStatus },
    );
  }

  await upsertThreadSessionState({
    companyId,
    persona,
    taskId,
    runId: result.runId ?? undefined,
    state: {
      mode: "skill_run_api",
      skill: skillSlug,
      loop_state: result.status === "running" ? "running" : "completed",
      compact_bytes: compact.bytes,
    },
  });

  return NextResponse.json({
    ok: true,
    skill: skillSlug,
    run_id: result.runId,
    status: result.status,
    execution_mode: result.executionMode,
    worker_evidence: result.workerEvidence,
    execution_verified: result.executionVerified,
    summary: result.summary,
    finalized: result.finalized,
    message: result.finalized
      ? `Worker run finalized (${result.status}, mode=${result.executionMode}).`
      : "Worker run dispatched; still running.",
  });
}
