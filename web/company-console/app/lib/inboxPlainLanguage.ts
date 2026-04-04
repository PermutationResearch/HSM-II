/** Same values the queue API accepts — keep in sync with `PolicyQueuePanel` filters. */
export type QueueView =
  | "all"
  | "overdue"
  | "atrisk"
  | "waiting_admin"
  | "pending_approvals"
  | "blocked";

/** One-line status people see instead of raw DB state. */
export function friendlyTaskState(state: string): string {
  const s = state.trim().toLowerCase().replace(/\s+/g, "_");
  const map: Record<string, string> = {
    open: "Open",
    todo: "To do",
    pending: "Waiting",
    in_progress: "In progress",
    doing: "In progress",
    active: "Active",
    done: "Done",
    complete: "Done",
    closed: "Closed",
    cancelled: "Cancelled",
    blocked: "Blocked",
    waiting_admin: "Needs you",
    failed: "Failed",
    error: "Error",
  };
  return map[s] ?? state.replace(/_/g, " ");
}

export function queueTabMeta(view: QueueView): { label: string; hint: string } {
  const m: Record<QueueView, { label: string; hint: string }> = {
    all: { label: "All", hint: "Everything in your list" },
    overdue: { label: "Late", hint: "Past when you wanted it done" },
    atrisk: { label: "At risk", hint: "Might become late soon" },
    waiting_admin: { label: "Needs you", hint: "Waiting on a person to act" },
    pending_approvals: { label: "Approvals", hint: "You need to say yes or no" },
    blocked: { label: "Stuck", hint: "Unblocked before work continues" },
  };
  return m[view];
}

export function friendlyPolicyDecision(mode: string): string {
  const s = mode.trim().toLowerCase();
  if (s === "auto" || s === "") return "Runs automatically";
  if (s === "admin_required") return "Needs your approval";
  if (s === "blocked") return "Not allowed automatically";
  return mode;
}

export function friendlyRisk(level: string): string {
  const s = level.trim().toLowerCase();
  const map: Record<string, string> = {
    low: "Low risk",
    medium: "Medium risk",
    high: "High risk",
    critical: "Critical risk",
  };
  return map[s] ?? level;
}
