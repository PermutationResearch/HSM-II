import { NextRequest, NextResponse } from "next/server";

import { parseRunLoopState } from "@/app/lib/runtime-contract";
import { UPSTREAM } from "@/app/lib/agent-chat-server";

type Body = {
  companyId?: string;
  runId?: string;
  interaction?: {
    kind?: "approval" | "elicitation";
    resume_token?: string;
    tool_name?: string;
    call_id?: string | null;
    message?: string;
  };
  action?: "approve" | "reject" | "respond";
  responseText?: string;
};

function asObj(v: unknown): Record<string, unknown> | null {
  return v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : null;
}

function toPending(meta: Record<string, unknown>): Array<Record<string, unknown>> {
  const arr = meta.pending_interactions;
  if (!Array.isArray(arr)) return [];
  return arr.map((x) => asObj(x)).filter((x): x is Record<string, unknown> => !!x);
}

export async function POST(req: NextRequest) {
  const body = (await req.json().catch(() => null)) as Body | null;
  const companyId = body?.companyId?.trim();
  const runId = body?.runId?.trim();
  const interaction = body?.interaction;
  const action = body?.action;
  const responseText = body?.responseText ?? "";

  if (!companyId || !runId || !interaction || !action) {
    return NextResponse.json(
      { error: "companyId, runId, interaction, action required" },
      { status: 400 },
    );
  }

  const kind = interaction.kind;
  const resumeToken = (interaction.resume_token ?? "").trim();
  if (!kind || !resumeToken) {
    return NextResponse.json(
      { error: "interaction.kind and interaction.resume_token required" },
      { status: 400 },
    );
  }

  const runRes = await fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${runId}`);
  if (!runRes.ok) {
    return NextResponse.json({ error: `run ${runId} not found` }, { status: 404 });
  }
  const runJson = (await runRes.json()) as { run?: { meta?: Record<string, unknown> } };
  const meta = (runJson.run?.meta ?? {}) as Record<string, unknown>;
  const pending = toPending(meta);

  try {
    if (kind === "approval") {
      const approvalKey =
        (asObj(meta.pending_approval_checkpoint)?.approval_key as string | undefined) ?? resumeToken;
      if (approvalKey) {
        await fetch(`${UPSTREAM}/api/approvals/decide`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            key: approvalKey,
            outcome: action === "reject" ? "deny" : "allow",
            actor: "operator_interaction",
          }),
        }).catch(() => {});
      }
    } else if (kind === "elicitation") {
      const executorBase = process.env.HSM_EXECUTOR_URL?.trim() || "http://127.0.0.1:4010";
      const elicitationAction = action === "reject" ? "reject" : action === "respond" ? "respond" : "accept";
      const payload: Record<string, unknown> = { action: elicitationAction };
      if (elicitationAction === "respond" || responseText.trim()) {
        payload.content = { text: responseText.trim() };
      }
      await fetch(`${executorBase.replace(/\/$/, "")}/elicit/respond/${encodeURIComponent(resumeToken)}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(payload),
      }).catch(() => {});
    }
  } catch (e) {
    return NextResponse.json(
      { error: e instanceof Error ? e.message : String(e) },
      { status: 502 },
    );
  }

  const nextPending = pending.filter((p) => String(p.resume_token ?? "") !== resumeToken);
  const loopState = parseRunLoopState(meta.loop_state) ?? "running";
  const nextLoop =
    nextPending.length > 0
      ? loopState
      : loopState === "waiting_approval" ||
          loopState === "waiting_elicitation" ||
          loopState === "checkpointed"
        ? "resuming"
        : loopState;

  await fetch(`${UPSTREAM}/api/company/companies/${companyId}/agent-runs/${runId}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      summary:
        action === "reject"
          ? `Interaction rejected for ${interaction.tool_name ?? "tool"}`
          : `Interaction resolved for ${interaction.tool_name ?? "tool"}`,
      meta: {
        ...meta,
        loop_state: nextLoop,
        needs_human: nextPending.length > 0,
        pending_interactions: nextPending,
        pending_approval_checkpoint: kind === "approval" ? null : meta.pending_approval_checkpoint ?? null,
        pending_elicitation_checkpoint:
          kind === "elicitation" ? null : meta.pending_elicitation_checkpoint ?? null,
      },
    }),
  }).catch(() => {});

  return NextResponse.json({
    ok: true,
    action,
    kind,
    run_id: runId,
    loop_state: nextLoop,
    pending_interactions: nextPending,
  });
}
