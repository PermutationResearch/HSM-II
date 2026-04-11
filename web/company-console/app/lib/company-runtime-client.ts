import type { HsmTaskRow } from "@/app/lib/hsm-api-types";
import { asArray, asObject, type AgentRunRecord } from "@/app/lib/runtime-contract";

type ApiErrorShape = { error?: string };

async function readJson(res: Response): Promise<unknown> {
  return res.json().catch(() => ({}));
}

function errorFromJson(json: unknown, fallback: string): string {
  const o = asObject(json);
  const e = o?.error;
  return typeof e === "string" && e.trim() ? e : fallback;
}

export async function getAgentRun(
  apiBase: string,
  companyId: string,
  runId: string,
): Promise<{ run: AgentRunRecord | null }> {
  const res = await fetch(`${apiBase}/api/company/companies/${companyId}/agent-runs/${runId}`);
  const json = await readJson(res);
  if (!res.ok) {
    throw new Error(errorFromJson(json, res.statusText));
  }
  const o = asObject(json);
  return { run: asObject(o?.run) as AgentRunRecord | null };
}

export async function listCompanyTasks(
  apiBase: string,
  companyId: string,
): Promise<{ tasks: HsmTaskRow[] }> {
  const res = await fetch(`${apiBase}/api/company/companies/${companyId}/tasks`);
  const json = await readJson(res);
  if (!res.ok) {
    throw new Error(errorFromJson(json, res.statusText));
  }
  const o = asObject(json);
  return { tasks: asArray(o?.tasks) as HsmTaskRow[] };
}

export async function patchAgentRun(
  apiBase: string,
  companyId: string,
  runId: string,
  patch: Record<string, unknown>,
): Promise<void> {
  const res = await fetch(`${apiBase}/api/company/companies/${companyId}/agent-runs/${runId}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(patch),
  });
  const json = await readJson(res);
  if (!res.ok) {
    throw new Error(errorFromJson(json, res.statusText));
  }
}

export async function postRunFeedback(
  apiBase: string,
  companyId: string,
  runId: string,
  payload: Record<string, unknown>,
): Promise<{ eventId?: string }> {
  const res = await fetch(`${apiBase}/api/company/companies/${companyId}/agent-runs/${runId}/feedback`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  const json = await readJson(res);
  if (!res.ok) {
    throw new Error(errorFromJson(json, res.statusText));
  }
  const o = asObject(json);
  const event = asObject(o?.event);
  return { eventId: typeof event?.id === "string" ? event.id : undefined };
}

export async function promoteRunFeedbackToTask(
  apiBase: string,
  companyId: string,
  runId: string,
  eventId: string,
  payload: Record<string, unknown>,
): Promise<{ taskId?: string; taskTitle?: string }> {
  const res = await fetch(
    `${apiBase}/api/company/companies/${companyId}/agent-runs/${runId}/feedback/${eventId}/promote-task`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    },
  );
  const json = await readJson(res);
  if (!res.ok) {
    throw new Error(errorFromJson(json, res.statusText));
  }
  const o = asObject(json);
  const task = asObject(o?.task);
  return {
    taskId: typeof task?.id === "string" ? task.id : undefined,
    taskTitle: typeof task?.title === "string" ? task.title : undefined,
  };
}

export async function markTaskRequiresHuman(
  apiBase: string,
  taskId: string,
  payload: Record<string, unknown>,
): Promise<void> {
  const res = await fetch(`${apiBase}/api/company/tasks/${taskId}/requires-human`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  const json = await readJson(res);
  if (!res.ok) {
    throw new Error(errorFromJson(json, res.statusText));
  }
}

export async function postTaskStigmergicNote(
  apiBase: string,
  taskId: string,
  payload: Record<string, unknown>,
): Promise<{ context_notes?: unknown }> {
  const res = await fetch(`${apiBase}/api/company/tasks/${taskId}/stigmergic-note`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  const json = await readJson(res);
  if (!res.ok) {
    throw new Error(errorFromJson(json, res.statusText));
  }
  const o = asObject(json);
  return { context_notes: o?.context_notes };
}

export async function createCompanyTask(
  apiBase: string,
  companyId: string,
  payload: Record<string, unknown>,
): Promise<{ taskId?: string }> {
  const res = await fetch(`${apiBase}/api/company/companies/${companyId}/tasks`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  const json = await readJson(res);
  if (!res.ok) {
    throw new Error(errorFromJson(json, res.statusText));
  }
  const o = asObject(json);
  const task = asObject(o?.task);
  return { taskId: typeof task?.id === "string" ? task.id : undefined };
}

export async function callResumeRun(payload: {
  companyId: string;
  runId: string;
  taskId: string;
  persona: string;
}): Promise<{ run_id?: string; status?: string; execution_mode?: string }> {
  const res = await fetch("/api/agent-runs/resume", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  const json = await readJson(res);
  if (!res.ok) {
    throw new Error(errorFromJson(json, res.statusText));
  }
  const o = asObject(json);
  return {
    run_id: typeof o?.run_id === "string" ? o.run_id : undefined,
    status: typeof o?.status === "string" ? o.status : undefined,
    execution_mode: typeof o?.execution_mode === "string" ? o.execution_mode : undefined,
  };
}

export function ensureOk(json: unknown): asserts json is { ok: true } & ApiErrorShape {
  const o = asObject(json);
  if (!o || o.ok !== true) {
    throw new Error(errorFromJson(json, "Request failed"));
  }
}

