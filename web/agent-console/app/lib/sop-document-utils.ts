import type { SopExampleDocument, SopGovernanceEvent, SopPhase } from "./sop-examples-types";

export function linesToList(text: string): string[] {
  return text
    .split("\n")
    .map((s) => s.trim())
    .filter(Boolean);
}

export function listToLines(items: string[]): string {
  return items.join("\n");
}

/** Stable id: letters, digits, underscore, hyphen. */
export function normalizeId(raw: string): string {
  const t = raw
    .trim()
    .replace(/\s+/g, "_")
    .replace(/[^a-zA-Z0-9_-]/g, "_")
    .replace(/_+/g, "_")
    .replace(/^_|_$/g, "")
    .slice(0, 80);
  return t || `custom_${Date.now().toString(36)}`;
}

export function emptyPhase(id?: string): SopPhase {
  return {
    id: id ?? "step_1",
    name: "New phase",
    actor: "both",
    sop_logic: "",
    actions: [""],
    company_os: [],
  };
}

export function emptyGovernanceEvent(): SopGovernanceEvent {
  return {
    action: "example_action",
    subject_type: "task",
    subject_hint: "task id",
    payload_summary: "{}",
  };
}

/** Default audit suggestions for “recommended” governance in the composer. */
export function defaultRecommendedInteractionLog(): SopExampleDocument["interaction_log"] {
  return {
    description:
      "Record when the workflow starts, important milestones, and when it finishes so leadership can follow the story—not only the outcome.",
    suggested_events: [
      {
        action: "sop.phase_started",
        subject_type: "task",
        subject_hint: "current task",
        payload_summary: "Phase began",
      },
      {
        action: "sop.step_completed",
        subject_type: "task",
        subject_hint: "current task",
        payload_summary: "Milestone reached",
      },
      {
        action: "sop.workflow_completed",
        subject_type: "task",
        subject_hint: "current task",
        payload_summary: "Workflow finished",
      },
    ],
  };
}

export function emptySopDocument(): SopExampleDocument {
  const id = `custom_${Date.now().toString(36)}`;
  return {
    kind: "hsm.sop_reference.v1",
    id,
    jsonFilename: `${id}.json`,
    tab_label: "New SOP",
    title: "Untitled playbook",
    summary: "Describe what this SOP achieves and when to use it.",
    phases: [emptyPhase("intake")],
    interaction_log: defaultRecommendedInteractionLog(),
  };
}

export function normalizeSopDocument(doc: SopExampleDocument): SopExampleDocument {
  const id = normalizeId(doc.id);
  const base = id.replace(/[^a-z0-9_-]+/gi, "-") || "sop";
  const fnRaw = doc.jsonFilename?.trim();
  const fn =
    fnRaw && fnRaw.endsWith(".json")
      ? fnRaw
      : fnRaw
        ? `${fnRaw.replace(/\.json$/i, "")}.json`
        : `${base}.json`;
  return {
    ...doc,
    id,
    kind: "hsm.sop_reference.v1",
    jsonFilename: fn.endsWith(".json") ? fn : `${fn}.json`,
    phases: doc.phases.map((p) => ({
      ...p,
      actions: Array.isArray(p.actions) ? p.actions : [],
      company_os: Array.isArray(p.company_os) ? p.company_os : [],
    })),
    interaction_log: {
      description: doc.interaction_log.description.trim() || "Governance log templates.",
      suggested_events: doc.interaction_log.suggested_events.map((e) => ({
        action: e.action.trim(),
        subject_type: e.subject_type.trim() || "task",
        subject_hint: e.subject_hint.trim(),
        payload_summary: e.payload_summary.trim(),
      })),
    },
  };
}

function canonicalGovernanceEvents(events: SopGovernanceEvent[]): string {
  return JSON.stringify(
    [...events]
      .map((e) => ({
        action: e.action.trim(),
        subject_type: e.subject_type.trim() || "task",
        subject_hint: e.subject_hint.trim(),
        payload_summary: e.payload_summary.trim(),
      }))
      .sort((a, b) => a.action.localeCompare(b.action))
  );
}

export function isRecommendedGovernanceLog(log: SopExampleDocument["interaction_log"]): boolean {
  return canonicalGovernanceEvents(log.suggested_events) === canonicalGovernanceEvents(defaultRecommendedInteractionLog().suggested_events);
}

/** Trim checklist lines, drop blank governance rows, fill missing phase ids from names. */
export function sanitizeSopDocument(doc: SopExampleDocument): SopExampleDocument {
  return {
    ...doc,
    phases: doc.phases.map((p, i) => {
      const idFromName = normalizeId(p.name);
      const pid = normalizeId(p.id) || idFromName || `step_${i + 1}`;
      return {
        ...p,
        id: pid,
        actions: (Array.isArray(p.actions) ? p.actions : []).map((t) => t.trim()).filter(Boolean),
        company_os: (Array.isArray(p.company_os) ? p.company_os : []).map((t) => t.trim()).filter(Boolean),
      };
    }),
    interaction_log: {
      description:
        doc.interaction_log.description.trim() ||
        "Log key moments so leadership can follow what happened.",
      suggested_events: doc.interaction_log.suggested_events
        .map((e) => ({
          action: e.action.trim(),
          subject_type: e.subject_type.trim() || "task",
          subject_hint: e.subject_hint.trim(),
          payload_summary: e.payload_summary.trim(),
        }))
        .filter((e) => e.action.length > 0),
    },
  };
}

export function validateSopDocument(
  doc: SopExampleDocument,
  opts?: { forbiddenIds?: Set<string> }
): string[] {
  const errs: string[] = [];
  if (!doc.title.trim()) errs.push('Add a title (e.g. "Monthly close checklist").');
  if (!doc.tab_label.trim()) errs.push("Add a short tab label—this is the name shown in the library.");
  if (!doc.id.trim()) errs.push("Internal id is missing; use Technical options to set or save again.");
  const nid = normalizeId(doc.id);
  if (opts?.forbiddenIds?.has(nid)) {
    errs.push(`That id is already used by a built-in example—pick another in Technical options.`);
  }
  if (doc.phases.length === 0) errs.push("Add at least one step.");
  for (const p of doc.phases) {
    if (!p.id.trim()) errs.push(`Step "${p.name || "?"}" needs a technical id—add a step name or open Advanced on that step.`);
    if (!p.name.trim()) errs.push("Each step needs a clear name.");
    if (p.actions.length === 0) errs.push(`Step "${p.name || p.id}": add at least one checklist item (what to do).`);
  }
  if (!doc.interaction_log.description.trim()) {
    errs.push("Add a one-line note about what leaders should see in the audit trail, or turn audit suggestions off.");
  }
  for (let i = 0; i < doc.interaction_log.suggested_events.length; i++) {
    const e = doc.interaction_log.suggested_events[i];
    if (!e.action.trim())
      errs.push(`Audit trail row ${i + 1} is incomplete—remove it or fill in the action in Technical options.`);
  }
  return errs;
}

export function parseSopDocumentJson(raw: string): SopExampleDocument {
  const j = JSON.parse(raw) as unknown;
  if (!j || typeof j !== "object") throw new Error("Not a JSON object");
  const o = j as Record<string, unknown>;
  if (o.kind !== "hsm.sop_reference.v1") throw new Error('Expected kind "hsm.sop_reference.v1"');
  return normalizeSopDocument(j as SopExampleDocument);
}
