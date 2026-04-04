import { Identity } from "./Identity";
import { timeAgo } from "../lib/timeAgo";
import { cn } from "../lib/utils";

/** Minimal event shape compatible with Paperclip-style rows (HSM maps governance → here). */
export interface ActivityEvent {
  id: string;
  action: string;
  createdAt: string;
  actorId: string;
  actorType: "agent" | "user" | "system";
  entityType: string;
  entityId: string;
  details?: Record<string, unknown> | null;
}

export interface AgentLite {
  id: string;
  name: string;
  /** From company_agents.title (registry). */
  title?: string;
  role?: string;
}

const ACTION_VERBS: Record<string, string> = {
  "issue.created": "created",
  "issue.updated": "updated",
  "governance.note": "noted on",
  "governance.apply": "applied on",
};

function humanizeValue(value: unknown): string {
  if (typeof value !== "string") return String(value ?? "none");
  return value.replace(/_/g, " ");
}

function formatVerb(action: string, details?: Record<string, unknown> | null): string {
  if (action === "issue.updated" && details) {
    const previous = (details._previous ?? {}) as Record<string, unknown>;
    if (details.status !== undefined) {
      const from = previous.status;
      return from
        ? `changed status from ${humanizeValue(from)} to ${humanizeValue(details.status)} on`
        : `changed status to ${humanizeValue(details.status)} on`;
    }
  }
  return ACTION_VERBS[action] ?? action.replace(/[._]/g, " ");
}

interface ActivityRowProps {
  event: ActivityEvent;
  agentMap: Map<string, AgentLite>;
  entityNameMap: Map<string, string>;
  entityTitleMap?: Map<string, string>;
  className?: string;
  /** When set, the row opens the related record in the main console (e.g. task in inbox). */
  onOpenSubject?: (entityType: string, entityId: string) => void;
}

export function ActivityRow({
  event,
  agentMap,
  entityNameMap,
  entityTitleMap,
  className,
  onOpenSubject,
}: ActivityRowProps) {
  const verb = formatVerb(event.action, event.details ?? undefined);
  const name = entityNameMap.get(`${event.entityType}:${event.entityId}`);
  const entityTitle = entityTitleMap?.get(`${event.entityType}:${event.entityId}`);
  const actor = event.actorType === "agent" ? agentMap.get(event.actorId) : null;
  const actorName =
    actor?.name ??
    (event.actorType === "system"
      ? "System"
      : event.actorType === "user"
        ? event.actorId || "Board"
        : event.actorId || "Unknown");

  const inner = (
    <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
      <span className="font-medium text-foreground">{actorName}</span>
      <span className="text-muted-foreground">{verb}</span>
      {name ? <span className="text-foreground">{name}</span> : null}
      {entityTitle ? <span className="text-muted-foreground">— {entityTitle}</span> : null}
      <span className="ml-auto text-xs text-muted-foreground">{timeAgo(event.createdAt)}</span>
    </div>
  );

  const classes = cn(
    "border-b border-border px-4 py-3 text-sm transition-colors last:border-b-0",
    onOpenSubject ? "hover:bg-muted/40" : "hover:bg-muted/30",
    className
  );

  const body = (
    <div className="flex gap-3">
      <Identity name={actorName} size="sm" />
      <div className="min-w-0 flex-1">{inner}</div>
    </div>
  );

  if (onOpenSubject) {
    return (
      <button
        type="button"
        className={cn(classes, "w-full cursor-pointer text-left")}
        onClick={() => onOpenSubject(event.entityType, event.entityId)}
      >
        {body}
      </button>
    );
  }

  return <div className={classes}>{body}</div>;
}
