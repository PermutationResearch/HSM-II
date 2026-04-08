"use client";

import { useRouter } from "next/navigation";
import { Dashboard, type DashboardDrillDown } from "@/ui/src/pages/Dashboard";
import { WorkspaceQuickStart } from "@/app/components/workspace/WorkspaceQuickStart";
import { useWorkspace } from "@/app/context/WorkspaceContext";

export default function WorkspaceDashboardPage() {
  const router = useRouter();
  const { apiBase, companyId, companies, companiesError, setPropertiesSelection } = useWorkspace();

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
