"use client";

import { useRouter } from "next/navigation";
import { Dashboard, type DashboardDrillDown } from "@/ui/src/pages/Dashboard";
import { WorkspaceQuickStart } from "@/app/components/workspace/WorkspaceQuickStart";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { useCompanyOpsOverview, useSelfImprovementSummary } from "@/app/lib/hsm-queries";

export default function WorkspaceDashboardPage() {
  const router = useRouter();
  const { apiBase, companyId, companies, companiesError, setPropertiesSelection } = useWorkspace();
  const { data: selfImprove } = useSelfImprovementSummary(apiBase, companyId);
  const { data: opsOverview } = useCompanyOpsOverview(apiBase, companyId);

  const handleDrillDown = (action: DashboardDrillDown) => {
    if (action.type === "task") {
      setPropertiesSelection({ kind: "task", id: action.taskId });
      router.push(`/workspace/issues?focus=${encodeURIComponent(action.taskId)}`);
      return;
    }
    if (action.type === "persona") {
      router.push("/workspace/agents");
      return;
    }
    if (action.type === "spend") {
      router.push("/workspace/costs");
      return;
    }

    const q = new URLSearchParams();
    if (action.type === "queue") q.set("view", action.view);
    if (action.type === "filter_priority") q.set("priority", String(action.level));
    if (action.type === "filter_state") q.set("state", action.state);
    if (action.type === "filter_task_ids") q.set("ids", action.ids.join(","));
    if (action.type === "filter_in_progress") q.set("filter", "in_progress");
    if (action.type === "filter_open") q.set("filter", "open");
    if (action.type === "filter_blocked") q.set("filter", "blocked");
    if (action.type === "filter_completed") q.set("filter", "completed");

    if (
      action.type === "inbox" ||
      action.type === "queue" ||
      action.type === "filter_priority" ||
      action.type === "filter_state" ||
      action.type === "filter_task_ids" ||
      action.type === "filter_in_progress" ||
      action.type === "filter_open" ||
      action.type === "filter_blocked" ||
      action.type === "filter_completed"
    ) {
      const qs = q.toString();
      router.push(qs ? `/workspace/issues?${qs}` : "/workspace/issues");
    }
  };

  if (companiesError) {
    return (
      <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-4 text-sm text-destructive-foreground">
        {companiesError.message}
      </div>
    );
  }

  return (
    <>
      <WorkspaceQuickStart />
      {opsOverview ? (
        <div className="mb-3 rounded-lg border border-admin-border bg-card px-4 py-2">
          <p className="font-mono text-[11px] text-muted-foreground">
            ROI snapshot · open tasks {opsOverview.overview.tasks_open} · requires human{" "}
            {opsOverview.overview.tasks_requires_human} · active agents {opsOverview.overview.agents_total} ·
            monthly spend ${opsOverview.overview.spend_total_usd.toFixed(2)} · avg tool prompt tokens{" "}
            {Math.round(opsOverview.audit.avg_tool_prompt_tokens ?? 0)}
            {opsOverview.roi
              ? ` · cycle ${opsOverview.roi.avg_cycle_time_hours_30d.toFixed(1)}h · retries/task ${opsOverview.roi.retries_per_task_7d.toFixed(2)} · closed/day ${opsOverview.roi.tasks_closed_per_day_14d.toFixed(2)}`
              : ""}
            {opsOverview.universality
              ? ` · profile ${opsOverview.universality.profile_business_model}/${opsOverview.universality.profile_size_tier} · setup ${(opsOverview.universality.setup_completion_rate * 100).toFixed(0)}% · c/resolved $${opsOverview.universality.cost_per_resolved_operation.toFixed(2)}`
              : ""}
          </p>
        </div>
      ) : null}
      {selfImprove ? (
        <div className="mb-4 rounded-lg border border-admin-border bg-card px-4 py-2 font-mono text-[11px] text-muted-foreground">
          Self-improvement 7d: failures {selfImprove.total_failures_7d} · first-pass{" "}
          {Math.round(selfImprove.first_pass_success_rate_7d * 100)}% · applied fixes{" "}
          {selfImprove.proposals_applied_7d}
        </div>
      ) : null}
      <Dashboard
      apiBase={apiBase}
      companyId={companyId}
      companies={companies.map((c) => ({
        id: c.id,
        display_name: c.display_name,
        issue_key_prefix: c.issue_key_prefix,
      }))}
      hrefAgents="/workspace/agents"
      hrefTasks="/workspace/issues"
      hrefCosts="/workspace/costs"
      hrefApprovals="/workspace/approvals"
      layout="admin"
      onDrillDown={handleDrillDown}
    />
    </>
  );
}
