"use client";

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/app/components/ui/card";
import { Badge } from "@/app/components/ui/badge";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { useMissionControl } from "@/app/lib/hsm-queries";

function boolLabel(v: unknown): string {
  return v === true ? "Yes" : "No";
}

export default function WorkspaceMissionControlPage() {
  const { apiBase, companyId } = useWorkspace();
  const { data, isLoading, error } = useMissionControl(apiBase, companyId);

  if (!companyId) {
    return <p className="pc-page-desc">Select a company in the header.</p>;
  }
  if (error) {
    return (
      <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-4 text-sm">
        {error instanceof Error ? error.message : String(error)}
      </div>
    );
  }

  const policy = data?.autonomy_policy ?? null;

  return (
    <div className="space-y-6">
      <div className="space-y-2 border-b border-admin-border pb-4">
        <p className="pc-page-eyebrow">Controls</p>
        <h1 className="pc-page-title">Guardrails</h1>
        <p className="max-w-3xl text-sm leading-relaxed text-muted-foreground">
          Set when your agents run, how much they can spend, and what triggers a pause for your review. These settings apply to all autonomous activity for this company.
        </p>
      </div>

      <Card className="border-admin-border bg-card/80">
        <CardHeader className="pb-3">
          <CardTitle className="text-base">Autonomous operation</CardTitle>
          <CardDescription>Turn autonomous execution on or off, set working hours, and cap how many tasks run at once.</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-2 text-sm md:grid-cols-2">
          <div>Agents running: <Badge variant="outline">{boolLabel((policy as any)?.autonomy_enabled)}</Badge></div>
          <div>Emergency stop: <Badge variant="outline">{boolLabel((policy as any)?.kill_switch)}</Badge></div>
          <div>Active hours (UTC): {(policy as any)?.quiet_hours_start_utc ?? 0}:00 → {(policy as any)?.quiet_hours_end_utc ?? 0}:00</div>
          <div>Max tasks at once: {(policy as any)?.max_concurrent_runs ?? 3}</div>
          <div>Daily spend limit: {(policy as any)?.daily_budget_usd ? `$${(policy as any).daily_budget_usd}` : "No limit set"}</div>
          <div>Updated by: {(policy as any)?.updated_by ?? "n/a"}</div>
        </CardContent>
      </Card>

      <Card className="border-admin-border bg-card/80">
        <CardHeader className="pb-3">
          <CardTitle className="text-base">Right now</CardTitle>
          <CardDescription>What's happening across your company at this moment.</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-3 text-sm md:grid-cols-3">
          <div className="rounded-lg border border-admin-border p-3">Waiting for your approval: {data?.ops.pending_approvals ?? 0}</div>
          <div className="rounded-lg border border-admin-border p-3">Active integrations: {data?.ops.running_connector_ops ?? 0}</div>
          <div className="rounded-lg border border-admin-border p-3">Scheduled tasks: {data?.ops.active_schedules ?? 0}</div>
          <div className="rounded-lg border border-admin-border p-3">Queued work: {data?.ops.automation_queue ?? 0}</div>
          <div className="rounded-lg border border-admin-border p-3">Issues in last 48h: {data?.ops.recent_incidents_48h ?? 0}</div>
          <div className="rounded-lg border border-admin-border p-3">{isLoading ? "Refreshing..." : "Updates live every 10s"}</div>
        </CardContent>
      </Card>

      <Card className="border-admin-border bg-card/80">
        <CardHeader className="pb-3">
          <CardTitle className="text-base">KPI Loop</CardTitle>
          <CardDescription>Latest KPI values and configured targets.</CardDescription>
        </CardHeader>
        <CardContent>
          {!data?.kpis?.length ? (
            <p className="text-sm text-muted-foreground">No KPI snapshots yet.</p>
          ) : (
            <div className="space-y-2 text-sm">
              {data.kpis.map((kpi) => (
                <div key={kpi.kpi_key} className="rounded-lg border border-admin-border p-3">
                  <div className="font-medium">{kpi.kpi_key}</div>
                  <div className="text-muted-foreground">
                    value={kpi.value} target={kpi.target_value ?? "n/a"} direction={kpi.direction ?? "n/a"}
                  </div>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
