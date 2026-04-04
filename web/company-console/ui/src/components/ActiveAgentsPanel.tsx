import { ExternalLink } from "lucide-react";
import type { HsmTask } from "../hooks/useHsmCompanyDashboard";
import { cn, relativeTime } from "../lib/utils";

function formatTaskKey(t: HsmTask, issueKeyPrefix: string | null | undefined): string {
  if (typeof t.display_number === "number") {
    return `${(issueKeyPrefix ?? "HSM").toUpperCase()}-${t.display_number}`;
  }
  return `HSM-${t.id.slice(0, 8)}`;
}

function isActiveTask(t: HsmTask): boolean {
  if (t.run?.status === "running") return true;
  const s = t.state.toUpperCase();
  return !!t.checked_out_by || s.includes("PROGRESS") || s === "OPEN";
}

const DEFAULT_FEED_HORIZON_MS = 72 * 60 * 60 * 1000;

function runFeedTimestamp(t: HsmTask): number {
  const r = t.run;
  if (!r) return 0;
  const iso = r.finished_at || r.updated_at;
  if (!iso) return 0;
  const n = Date.parse(iso);
  return Number.isFinite(n) ? n : 0;
}

/** Live strip ordering: running → checked out → in progress → open. */
function feedRank(t: HsmTask): number {
  if (t.run?.status === "running") return 0;
  if (t.checked_out_by) return 1;
  if (/progress/i.test(t.state)) return 2;
  if (t.state.toUpperCase() === "OPEN") return 3;
  return 4;
}

/**
 * Paperclip-style feed: live checkout / in-flight work first, then recent terminal runs
 * (success/error), then idle rows with fresh tool/log activity.
 */
export function selectAgentFeedTasks(
  tasks: HsmTask[],
  maxItems: number,
  recentHorizonMs: number = DEFAULT_FEED_HORIZON_MS,
  nowMs: number = Date.now(),
): HsmTask[] {
  const chosen = new Set<string>();
  const out: HsmTask[] = [];

  const push = (t: HsmTask) => {
    if (chosen.has(t.id) || out.length >= maxItems) return;
    chosen.add(t.id);
    out.push(t);
  };

  const live = tasks.filter(isActiveTask).sort((a, b) => feedRank(a) - feedRank(b));
  for (const t of live) push(t);

  const recentDone = tasks
    .filter((t) => {
      if (chosen.has(t.id)) return false;
      const st = t.run?.status;
      if (st !== "success" && st !== "error") return false;
      const ts = runFeedTimestamp(t);
      return ts > 0 && nowMs - ts <= recentHorizonMs;
    })
    .sort((a, b) => runFeedTimestamp(b) - runFeedTimestamp(a));
  for (const t of recentDone) push(t);

  const recentIdle = tasks
    .filter((t) => {
      if (chosen.has(t.id)) return false;
      const r = t.run;
      if (!r || r.status !== "idle") return false;
      const ts = r.updated_at ? Date.parse(r.updated_at) : 0;
      if (!Number.isFinite(ts) || nowMs - ts > recentHorizonMs) return false;
      return r.tool_calls > 0 || !!(r.log_tail && r.log_tail.trim());
    })
    .sort((a, b) => {
      const ua = a.run?.updated_at ? Date.parse(a.run.updated_at) : 0;
      const ub = b.run?.updated_at ? Date.parse(b.run.updated_at) : 0;
      return ub - ua;
    });
  for (const t of recentIdle) push(t);

  return out;
}

/**
 * Paperclip "Agents" live strip layout — upstream uses heartbeats API + run logs;
 * HSM uses checked-out / in-progress tasks and optional `company_agents` registry labels.
 */
export function ActiveAgentsPanel({
  tasks,
  layout = "nothing",
  agentRegistry = {},
  issueKeyPrefix,
  onTaskClick,
  maxFeedItems = 8,
  recentHorizonHours = 72,
}: {
  tasks: HsmTask[];
  layout?: "nothing" | "admin";
  /** Keys: agent id / persona name → title & role from workforce registry. */
  agentRegistry?: Record<string, { title?: string; role?: string }>;
  issueKeyPrefix?: string | null;
  /** Opens inbox focused on this task (spec / SLA / checkout). */
  onTaskClick?: (taskId: string) => void;
  /** Max cards: live first, then recent finished / telemetry (default 8). */
  maxFeedItems?: number;
  /** How far back to show non-live runs with terminal or idle telemetry (default 72). */
  recentHorizonHours?: number;
}) {
  const horizonMs = recentHorizonHours * 60 * 60 * 1000;
  const runs = selectAgentFeedTasks(tasks, maxFeedItems, horizonMs);

  return (
    <div className="space-y-3">
      <div className="flex flex-col gap-0.5 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h2
            className={
              layout === "admin"
                ? "font-mono text-[12px] font-semibold uppercase tracking-[0.08em] text-[#C9D1D9]"
                : "nd-label"
            }
          >
            Agents
          </h2>
          <p
            className={
              layout === "admin"
                ? "mt-0.5 font-mono text-[10px] uppercase tracking-wide text-[#6E7681]"
                : "mt-0.5 text-[11px] text-muted-foreground"
            }
          >
            Live checkout and runs from the last {recentHorizonHours}h
          </p>
        </div>
      </div>
      {runs.length === 0 ? (
        <p
          className={
            layout === "admin"
              ? "rounded-2xl bg-[#0a0a0a] px-4 py-6 text-center text-sm text-[#8B949E]"
              : "rounded-2xl border border-dashed border-border bg-muted/30 px-4 py-6 text-center text-sm text-muted-foreground"
          }
        >
          No recent agent runs.
        </p>
      ) : (
        <div
          className={
            layout === "admin"
              ? "grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4"
              : "grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4"
          }
        >
          {runs.map((t) => (
            <AgentRunCard
              key={t.id}
              task={t}
              layout={layout}
              agentRegistry={agentRegistry}
              issueKeyPrefix={issueKeyPrefix}
              onTaskClick={onTaskClick}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function AgentRunCard({
  task,
  layout = "nothing",
  agentRegistry,
  issueKeyPrefix,
  onTaskClick,
}: {
  task: HsmTask;
  layout?: "nothing" | "admin";
  agentRegistry: Record<string, { title?: string; role?: string }>;
  issueKeyPrefix?: string | null;
  onTaskClick?: (taskId: string) => void;
}) {
  const persona = task.owner_persona ?? task.checked_out_by ?? "";
  const reg = persona ? agentRegistry[persona] : undefined;
  const runStatus = task.run?.status;
  const isLive =
    runStatus === "running" ||
    !!task.checked_out_by ||
    /progress/i.test(task.state);
  const runFailed = runStatus === "error";
  const runSucceeded = runStatus === "success";
  const snippet =
    (task.specification && task.specification.trim().slice(0, 160)) ||
    task.title.slice(0, 120) ||
    "—";
  const logTail = (task.run?.log_tail ?? "").trim();
  const toolCalls = typeof task.run?.tool_calls === "number" ? task.run.tool_calls : 0;
  const finishedAt = task.run?.finished_at ?? null;
  const shell = cn(
    "rounded-2xl p-4 text-left text-card-foreground transition-colors duration-200 ease-out",
    layout === "admin" ? "bg-[#0a0a0a]" : "border border-border bg-card",
    onTaskClick &&
      (layout === "admin"
        ? "cursor-pointer hover:bg-[#111111]"
        : "cursor-pointer hover:border-[#333333] hover:bg-[#1A1A1A]")
  );
  const inner = (
    <>
      <div className="mb-2 flex items-start justify-between gap-2">
        <div className="min-w-0">
          <p className="truncate font-mono text-[11px] font-normal uppercase tracking-[0.06em] text-[#999999]">
            {persona || "Agent"}
            {reg?.title ? (
              <span className="ml-1 font-sans text-[10px] font-normal normal-case tracking-normal text-[#8B949E]">
                — {reg.title}
              </span>
            ) : null}
            {isLive ? (
              <span className="ml-2 rounded bg-[#4A9E5C]/15 px-1.5 py-0.5 text-[10px] font-semibold normal-case tracking-normal text-[#7EE787]">
                LIVE
              </span>
            ) : runFailed ? (
              <span className="ml-2 rounded bg-[#D71921]/12 px-1.5 py-0.5 text-[10px] font-semibold normal-case tracking-normal text-[#FF7B72]">
                FAILED
              </span>
            ) : runSucceeded ? (
              <span className="ml-2 rounded bg-[#388BFD]/10 px-1.5 py-0.5 text-[10px] font-semibold normal-case tracking-normal text-[#79C0FF]">
                DONE
              </span>
            ) : (
              <span className="ml-2 text-[10px] font-normal normal-case tracking-normal text-[#666666]">RECENT</span>
            )}
          </p>
          <p className="mt-1 line-clamp-2 text-sm font-medium leading-snug text-foreground">{task.title}</p>
        </div>
        <ExternalLink className="h-3.5 w-3.5 shrink-0 text-muted-foreground" aria-hidden />
      </div>
      <p className="text-[11px] text-muted-foreground">
        {isLive ? (
          <span className="inline-flex items-center gap-1.5 font-medium text-[#4A9E5C]">
            <span className="h-2 w-2 rounded-full bg-[#4A9E5C]" />
            LIVE NOW
          </span>
        ) : runFailed ? (
          <span className="text-[#F85149]">
            Failed {finishedAt ? `· ${relativeTime(finishedAt)}` : task.due_at ? `· ${relativeTime(task.due_at)}` : ""}
          </span>
        ) : runSucceeded ? (
          <span className="text-[#8B949E]">
            Succeeded
            {finishedAt
              ? ` · ${relativeTime(finishedAt)}`
              : task.run?.updated_at
                ? ` · ${relativeTime(task.run.updated_at)}`
                : ""}
          </span>
        ) : (
          <span>
            Updated{" "}
            {task.run?.updated_at
              ? relativeTime(task.run.updated_at)
              : finishedAt
                ? relativeTime(finishedAt)
                : task.due_at
                  ? relativeTime(task.due_at)
                  : "recently"}
          </span>
        )}
      </p>
      {toolCalls > 0 ? (
        <p className="mt-1 font-mono text-[10px] text-[#8B949E]">{toolCalls} tool call{toolCalls === 1 ? "" : "s"}</p>
      ) : null}
      <div className="mt-3 overflow-hidden rounded-lg border border-[#333333] bg-[#000000]">
        <div className="flex items-center gap-1.5 border-b border-[#222222] px-2 py-1">
          <span className="h-1.5 w-1.5 rounded-full bg-[#D71921]" />
          <span className="h-1.5 w-1.5 rounded-full bg-[#D4A843]" />
          <span className="h-1.5 w-1.5 rounded-full bg-[#4A9E5C]" />
          <span className="flex-1" />
          <span className="font-mono text-[9px] uppercase tracking-wide text-[#666666]">agent</span>
        </div>
        <pre className="max-h-[72px] overflow-hidden p-2 font-mono text-[9px] leading-relaxed text-[#999999] whitespace-pre-wrap break-words">
          {logTail ? (
            logTail.length > 2000 ? `${logTail.slice(-2000)}` : logTail
          ) : isLive ? (
            <>
              <span className="text-white">$ </span>
              Awaiting log output — task context below
              {"\n"}
              <span className="opacity-70">{snippet}</span>
              {snippet.length >= 120 ? "…" : ""}
            </>
          ) : (
            snippet
          )}
        </pre>
      </div>
      <p className="mt-2 font-mono text-[10px] text-muted-foreground">
        {formatTaskKey(task, issueKeyPrefix)}
        {task.checked_out_by ? ` · ${task.checked_out_by}` : ""}
        {runStatus && runStatus !== "idle" ? ` · run:${runStatus}` : ""}
      </p>
    </>
  );
  if (onTaskClick) {
    return (
      <button type="button" className={cn(shell, "w-full")} onClick={() => onTaskClick(task.id)}>
        {inner}
      </button>
    );
  }
  return <div className={shell}>{inner}</div>;
}
