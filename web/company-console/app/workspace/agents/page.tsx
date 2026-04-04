"use client";

import { Badge } from "@/app/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/app/components/ui/card";
import { ScrollArea } from "@/app/components/ui/scroll-area";
import { Skeleton } from "@/app/components/ui/skeleton";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { toPcWorkforceAgent } from "@/app/lib/hsm-api-adapter";
import { useCompanyAgents } from "@/app/lib/hsm-queries";

export default function WorkspaceAgentsPage() {
  const { apiBase, companyId, setPropertiesSelection } = useWorkspace();
  const { data: agents = [], isLoading, error } = useCompanyAgents(apiBase, companyId);

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

  const rows = agents.filter((a) => a.status !== "terminated").map(toPcWorkforceAgent);

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-lg font-semibold tracking-tight text-foreground">Agents</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Workforce registry (<span className="font-mono text-xs">company_agents</span>) — Paperclip-style roster backed by HSM
          endpoints.
        </p>
      </div>

      <Card className="border-admin-border bg-card">
        <CardHeader className="pb-2">
          <CardTitle className="text-base">Workforce</CardTitle>
          <CardDescription>{isLoading ? "Loading…" : `${rows.length} agents`}</CardDescription>
        </CardHeader>
        <CardContent className="p-0">
          <ScrollArea className="h-[min(70vh,560px)]">
            <div className="min-w-[640px]">
              <div className="grid grid-cols-[1.2fr_1fr_1fr_100px_100px] gap-2 border-b border-admin-border bg-muted/30 px-4 py-2 font-mono text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
                <span>Name</span>
                <span>Role</span>
                <span>Title</span>
                <span>Status</span>
                <span>Budget/mo</span>
              </div>
              {isLoading
                ? Array.from({ length: 6 }).map((_, i) => (
                    <div key={i} className="flex gap-2 border-b border-admin-border px-4 py-3">
                      <Skeleton className="h-4 flex-1" />
                      <Skeleton className="h-4 w-24" />
                    </div>
                  ))
                : rows.map((a) => (
                    <button
                      key={a.id}
                      type="button"
                      className="pc-table-row grid w-full grid-cols-[1.2fr_1fr_1fr_100px_100px] gap-2"
                      onClick={() => setPropertiesSelection({ kind: "agent", id: a.id, name: a.name })}
                    >
                      <span className="font-mono text-xs text-primary">{a.name}</span>
                      <span className="text-muted-foreground">{a.role}</span>
                      <span className="truncate text-muted-foreground">{a.title ?? "—"}</span>
                      <span>
                        <Badge variant="outline" className="font-mono text-[10px] uppercase">
                          {a.status}
                        </Badge>
                      </span>
                      <span className="font-mono text-xs text-muted-foreground">
                        {a.budgetMonthlyCents != null ? `$${(a.budgetMonthlyCents / 100).toFixed(2)}` : "—"}
                      </span>
                    </button>
                  ))}
            </div>
          </ScrollArea>
        </CardContent>
      </Card>
    </div>
  );
}
