"use client";

import { useMemo } from "react";
import { AlertTriangle, CheckCircle2, Clock, UserCheck } from "lucide-react";
import { Badge } from "@/app/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/app/components/ui/card";
import { ScrollArea } from "@/app/components/ui/scroll-area";
import { Skeleton } from "@/app/components/ui/skeleton";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { taskToPcIssue } from "@/app/lib/hsm-api-adapter";
import type { HsmTaskRow } from "@/app/lib/hsm-api-types";
import { useCompanyAgents, useCompanyTasks } from "@/app/lib/hsm-queries";

// ── Heuristics ────────────────────────────────────────────────────────────────

function isInReview(t: HsmTaskRow): boolean {
  return /review|approval|pending_review/i.test(t.state) || t.decision_mode === "admin_required";
}

function isAssignedToMe(t: HsmTaskRow, myAgentIds: Set<string>): boolean {
  const who = t.owner_persona ?? t.checked_out_by ?? "";
  return myAgentIds.has(who);
}

function isFailed(t: HsmTaskRow): boolean {
  return /fail|error|timeout|abort/i.test(t.state) || t.run?.status === "failed";
}

// ── Sub-components ────────────────────────────────────────────────────────────

type SectionProps = {
  title: string;
  icon: React.ReactNode;
  tasks: HsmTaskRow[];
  issuePrefix: string;
  isLoading: boolean;
  onSelect: (t: HsmTaskRow, label: string) => void;
  emptyText: string;
  badgeVariant?: "destructive" | "secondary" | "outline" | "default";
};

function WorkSection({
  title,
  icon,
  tasks,
  issuePrefix,
  isLoading,
  onSelect,
  emptyText,
  badgeVariant = "secondary",
}: SectionProps) {
  return (
    <Card className="border-admin-border bg-card">
      <CardHeader className="pb-2 pt-3">
        <CardTitle className="flex items-center gap-2 text-sm font-semibold">
          {icon}
          {title}
          {!isLoading && (
            <Badge
              variant={tasks.length > 0 ? badgeVariant : "outline"}
              className="ml-auto font-mono text-[10px]"
            >
              {tasks.length}
            </Badge>
          )}
        </CardTitle>
      </CardHeader>
      <CardContent className="p-0">
        <ScrollArea className="h-[min(40vh,320px)]">
          {isLoading ? (
            <div className="space-y-1 p-3">
              {Array.from({ length: 3 }).map((_, i) => (
                <Skeleton key={i} className="h-9 w-full rounded" />
              ))}
            </div>
          ) : tasks.length === 0 ? (
            <p className="px-4 py-4 text-xs text-muted-foreground">{emptyText}</p>
          ) : (
            <div>
              {tasks.map((t) => {
                const issue = taskToPcIssue(t, issuePrefix);
                return (
                  <button
                    key={t.id}
                    type="button"
                    className="pc-table-row flex w-full items-center gap-2 px-4 py-2.5"
                    onClick={() => onSelect(t, `${issue.identifier} · ${t.title}`)}
                  >
                    <span className="font-mono text-[10px] text-primary">{issue.identifier}</span>
                    <span className="flex-1 truncate text-left text-xs font-medium text-foreground">
                      {t.title}
                    </span>
                    <Badge variant="outline" className="shrink-0 font-mono text-[9px] uppercase">
                      {t.state}
                    </Badge>
                  </button>
                );
              })}
            </div>
          )}
        </ScrollArea>
      </CardContent>
    </Card>
  );
}

// ── Page ──────────────────────────────────────────────────────────────────────

export default function MyWorkPage() {
  const { apiBase, companyId, companies, setPropertiesSelection } = useWorkspace();
  const company = companies.find((c) => c.id === companyId);
  const prefix = (company?.issue_key_prefix ?? "HSM").toUpperCase();

  const { data: tasks = [], isLoading: tasksLoading } = useCompanyTasks(apiBase, companyId);
  const { data: agents = [], isLoading: agentsLoading } = useCompanyAgents(apiBase, companyId);

  // "My" agents: all active agents in this company act as "me" in a shared workspace.
  // When personal identity is available (e.g. auth), filter to just the current user's agents.
  const myAgentIds = useMemo(
    () => new Set(agents.filter((a) => a.status === "active").map((a) => a.name)),
    [agents],
  );

  const isLoading = tasksLoading || agentsLoading;

  const live = tasks.filter((t) => t.state !== "terminated" && t.state !== "done" && t.state !== "closed");

  const inReview = useMemo(() => live.filter(isInReview), [live]);
  const assignedToMe = useMemo(() => live.filter((t) => isAssignedToMe(t, myAgentIds)), [live, myAgentIds]);
  const failed = useMemo(() => tasks.filter(isFailed), [tasks]);

  // "Recently touched" = tasks that have a run with a recent updated_at / finished_at, or checked out now
  const recentlyTouched = useMemo(() => {
    const cutoff = Date.now() - 24 * 60 * 60 * 1000; // last 24 hours
    return tasks
      .filter((t) => {
        const ts =
          t.run?.updated_at ?? t.run?.finished_at ?? t.checked_out_until ?? null;
        if (!ts) return false;
        return new Date(ts).getTime() > cutoff;
      })
      .slice(0, 20);
  }, [tasks]);

  function handleSelect(t: HsmTaskRow, label: string) {
    setPropertiesSelection({ kind: "task", id: t.id, title: label });
  }

  if (!companyId) {
    return <p className="pc-page-desc">Select a company in the header.</p>;
  }

  return (
    <div className="space-y-5">
      <div>
        <p className="pc-page-eyebrow">Workspace</p>
        <h1 className="pc-page-title">My Work</h1>
        <p className="pc-page-desc">Tasks that need your attention across this company.</p>
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <WorkSection
          title="In Review"
          icon={<CheckCircle2 className="size-4 text-primary" strokeWidth={1.5} />}
          tasks={inReview}
          issuePrefix={prefix}
          isLoading={isLoading}
          onSelect={handleSelect}
          emptyText="Nothing awaiting review."
          badgeVariant="outline"
        />

        <WorkSection
          title="Assigned to Me"
          icon={<UserCheck className="size-4 text-emerald-500" strokeWidth={1.5} />}
          tasks={assignedToMe}
          issuePrefix={prefix}
          isLoading={isLoading}
          onSelect={handleSelect}
          emptyText="No tasks currently assigned to active agents."
          badgeVariant="secondary"
        />

        <WorkSection
          title="Recently Touched"
          icon={<Clock className="size-4 text-amber-400" strokeWidth={1.5} />}
          tasks={recentlyTouched}
          issuePrefix={prefix}
          isLoading={isLoading}
          onSelect={handleSelect}
          emptyText="No activity in the last 24 hours."
          badgeVariant="outline"
        />

        <WorkSection
          title="Failed Runs"
          icon={<AlertTriangle className="size-4 text-destructive" strokeWidth={1.5} />}
          tasks={failed}
          issuePrefix={prefix}
          isLoading={isLoading}
          onSelect={handleSelect}
          emptyText="No failed runs."
          badgeVariant="destructive"
        />
      </div>
    </div>
  );
}
