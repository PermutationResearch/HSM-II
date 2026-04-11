import { NextRequest, NextResponse } from "next/server";
import { parseRunLoopState } from "@/app/lib/runtime-contract";

import {
  dispatchSkillToWorker,
  patchRunLoopState,
  resolveAgentForPersona,
  UPSTREAM,
  upsertThreadSessionState,
} from "@/app/lib/agent-chat-server";

type Body = {
  companyId?: string;
  runId?: string;
  taskId?: string;
  persona?: string;
};

export async function POST(req: NextRequest) {
  const body = (await req.json().catch(() => null)) as Body | null;
  const companyId = body?.companyId?.trim();
  const runId = body?.runId?.trim();
  const taskId = body?.taskId?.trim();
  const persona = body?.persona?.trim();
  if (!companyId || !runId || !taskId || !persona) {
    return NextResponse.json({ error: "companyId, runId, taskId, persona required" }, { status: 400 });
  }

  const runRes = await fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${runId}`);
  if (!runRes.ok) {
    return NextResponse.json({ error: `run ${runId} not found` }, { status: 404 });
  }
  const runJson = (await runRes.json()) as { run?: { meta?: Record<string, unknown>; summary?: string | null } };
  const meta = runJson.run?.meta ?? {};
  const fromLoopState = parseRunLoopState(meta.loop_state) ?? "paused_approval";
  const checkpoint = (meta.pending_approval_checkpoint ?? null) as
    | { tool_name?: string; call_id?: string; message?: string; approval_key?: string; execution_id?: string }
    | null;

  if (checkpoint?.approval_key) {
    await fetch(`${UPSTREAM}/api/approvals/decide`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        key: checkpoint.approval_key,
        outcome: "allow",
        actor: "operator_resume",
      }),
    }).catch(() => {});
  }
  if (checkpoint?.execution_id) {
    await fetch(
      `${UPSTREAM}/api/company/companies/${companyId}/tools/executions/${encodeURIComponent(checkpoint.execution_id)}/resume`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ actor: "operator_resume" }),
      },
    ).catch(() => {});
  }

  const { allKnownSlugs } = await resolveAgentForPersona(companyId, persona);
  const skillRaw = String(meta.skill ?? "operator-chat");
  const skill =
    allKnownSlugs.includes(skillRaw) || skillRaw === "operator-chat" ? skillRaw : "operator-chat";

  const replayInstruction = checkpoint
    ? `Resume blocked tool action ${checkpoint.tool_name ?? "tool"} (${checkpoint.call_id ?? "n/a"}) and continue.`
    : "Resume paused run and continue.";
  await fetch(`${UPSTREAM}/api/company/tasks/${taskId}/stigmergic-note`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ actor: "operator", text: replayInstruction }),
  }).catch(() => {});

  const result = await dispatchSkillToWorker({
    companyId,
    taskId,
    persona,
    skillSlug: skill,
    externalSystem: "worker-dispatch-resume",
    persistAgentNote: true,
    runSummary: `Resumed run ${runId}: ${replayInstruction}`,
    extraMeta: {
      resumed_from_run_id: runId,
      resume_checkpoint: checkpoint ?? null,
      loop_state: "running",
      execution_mode: "pending",
    },
  });
  if (!result.ok) {
    return NextResponse.json({ error: result.error, run_id: result.runId ?? undefined }, { status: result.httpStatus });
  }

  await patchRunLoopState({
    companyId,
    runId,
    currentMeta: meta,
    from: fromLoopState === "paused_auth" ? "paused_auth" : "paused_approval",
    to: "resumed",
    extraMeta: { resumed_into_run_id: result.runId, pending_approval_checkpoint: null },
  }).catch(() => false);

  await fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${runId}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      status: "cancelled",
      summary: `Paused run resumed as ${result.runId}.`,
      finished_at: true,
      meta: {
        ...meta,
        loop_state: "resumed",
        resumed_into_run_id: result.runId,
        pending_approval_checkpoint: null,
      },
    }),
  }).catch(() => {});

  await upsertThreadSessionState({
    companyId,
    persona,
    taskId,
    runId: result.runId ?? undefined,
    state: {
      mode: "resume",
      resumed_from: runId,
      loop_state: result.status === "running" ? "running" : "completed",
    },
  });

  return NextResponse.json({
    ok: true,
    resumed_from_run_id: runId,
    run_id: result.runId,
    status: result.status,
    execution_mode: result.executionMode,
    finalized: result.finalized,
  });
}

