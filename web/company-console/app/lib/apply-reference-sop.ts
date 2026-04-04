import type { SopExampleDocument, SopPhase } from "./sop-examples-types";

const GOV_ACTOR = "sop_reference_ui";

function extractOwnerPersona(phase: SopPhase): string | undefined {
  const re = /owner_persona\s+([a-zA-Z0-9_-]+)/i;
  for (const line of phase.actions) {
    const m = line.match(re);
    if (m) return m[1];
  }
  return undefined;
}

function buildPhaseSpec(ex: SopExampleDocument, phase: SopPhase): string {
  const lines: string[] = [
    `**SOP:** ${ex.title} (\`${ex.id}\`)`,
    `**Phase:** ${phase.name} (\`${phase.id}\`) · actor: ${phase.actor}`,
    "",
    "### Logic",
    phase.sop_logic,
    "",
    "### Actions",
    ...phase.actions.map((a) => `- ${a}`),
    "",
    "### Company OS hooks",
    ...phase.company_os.map((c) => `- ${c}`),
  ];
  if (phase.resolution) {
    lines.push("", "### Resolution", phase.resolution);
  }
  if (phase.escalation) {
    lines.push("", "### Escalation", phase.escalation);
  }
  return lines.join("\n");
}

function buildParentSpec(ex: SopExampleDocument): string {
  return [
    "Materialized from the **Reference SOP** library: one child task per phase (checklist / playbook).",
    "Suggested governance events were seeded as templates (`seeded_from_reference: true`).",
    "",
    ex.summary,
    "",
    `**Phases:** ${ex.phases.map((p) => p.name).join(" → ")}`,
  ].join("\n");
}

type TaskCreateRes = { task?: { id: string }; error?: string };

async function postTask(
  apiBase: string,
  companyId: string,
  body: Record<string, unknown>
): Promise<string> {
  const r = await fetch(`${apiBase}/api/company/companies/${companyId}/tasks`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const j = (await r.json()) as TaskCreateRes;
  if (!r.ok) throw new Error(j.error ?? `create task ${r.status}`);
  const id = j.task?.id;
  if (!id) throw new Error("API returned no task id");
  return id;
}

async function postGov(
  apiBase: string,
  companyId: string,
  body: Record<string, unknown>
): Promise<void> {
  const r = await fetch(`${apiBase}/api/company/companies/${companyId}/governance/events`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const j = (await r.json()) as { error?: string };
  if (!r.ok) throw new Error(j.error ?? `governance ${r.status}`);
}

/**
 * Creates a parent playbook task, phase tasks (linked via `parent_task_id`), bumps priority on escalation-like phases,
 * posts `sop_playbook_materialized`, then seeds suggested governance rows (marked as reference templates).
 */
export async function applyReferenceSopToCompany(opts: {
  apiBase: string;
  companyId: string;
  document: SopExampleDocument;
}): Promise<{ parent_task_id: string; phase_task_ids: string[] }> {
  const apiBase = opts.apiBase.replace(/\/$/, "");
  const { companyId, document } = opts;

  const parentId = await postTask(apiBase, companyId, {
    title: `SOP playbook · ${document.tab_label}`,
    specification: buildParentSpec(document),
  });

  const phaseIds: string[] = [];
  for (const phase of document.phases) {
    const tid = await postTask(apiBase, companyId, {
      title: `Phase · ${phase.name}`,
      specification: buildPhaseSpec(document, phase),
      owner_persona: extractOwnerPersona(phase),
      parent_task_id: parentId,
    });
    phaseIds.push(tid);

    if (/escalat/i.test(phase.id) || /escalat/i.test(phase.name)) {
      const sr = await fetch(`${apiBase}/api/company/tasks/${tid}/sla`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ priority: 3 }),
      });
      if (!sr.ok) {
        /* optional: escalation priority best-effort */
      }
    }
  }

  await postGov(apiBase, companyId, {
    actor: GOV_ACTOR,
    action: "sop_playbook_materialized",
    subject_type: "task",
    subject_id: parentId,
    payload: {
      sop_id: document.id,
      sop_title: document.title,
      phase_task_ids: phaseIds,
    },
  });

  const events = document.interaction_log.suggested_events;
  for (let i = 0; i < events.length; i++) {
    const ev = events[i];
    const subjectId = phaseIds.length === events.length ? phaseIds[i]! : parentId;
    await postGov(apiBase, companyId, {
      actor: GOV_ACTOR,
      action: ev.action,
      subject_type: ev.subject_type,
      subject_id: subjectId,
      payload: {
        seeded_from_reference: true,
        summary: ev.payload_summary,
        hint: ev.subject_hint,
      },
    });
  }

  return { parent_task_id: parentId, phase_task_ids: phaseIds };
}
