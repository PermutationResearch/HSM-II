/** Paperclip-style task spec lines for workspace file pointers (relative to company `hsmii_home`). */
import { capabilityRefsFromTask } from "@/app/components/TaskListPanel";
import type { HsmTaskRow } from "@/app/lib/hsm-api-types";

export const workspaceFileLine = (path: string) => `Workspace file: ${path}`;

/** Merge `Workspace file: …` lines into spec without duplicating exact lines. */
export function specificationWithWorkspacePaths(spec: string, paths: string[]): string {
  let s = spec.trimEnd();
  for (const p of paths) {
    const line = workspaceFileLine(p);
    if (!s.includes(line)) {
      s += (s ? "\n\n" : "") + line;
    }
  }
  return s;
}

export function truncatePath(p: string, max = 56): string {
  if (p.length <= max) return p;
  return `${p.slice(0, max - 1)}…`;
}

export function isPlanTask(task: Pick<HsmTaskRow, "capability_refs">): boolean {
  return capabilityRefsFromTask(task).some((c) => c.kind === "mode" && c.ref === "plan");
}

export function isDoneTask(task: Pick<HsmTaskRow, "state">): boolean {
  return /done|complete|closed/i.test(task.state);
}

export function buildIssueTitleFromPlan(title: string): string {
  return `Build: ${title}`;
}

export function buildIssueSpecFromPlan(specification?: string | null): string {
  return specification ?? "";
}
