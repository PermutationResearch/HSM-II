"use client";

import Link from "next/link";
import { ChevronRight } from "lucide-react";
import { Badge } from "@/app/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/app/components/ui/card";
import { Skeleton } from "@/app/components/ui/skeleton";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { toPcWorkforceAgent } from "@/app/lib/hsm-api-adapter";
import { useCompanyAgents } from "@/app/lib/hsm-queries";

export default function WorkspaceAgentsPage() {
  const { apiBase, companyId, setPropertiesSelection } = useWorkspace();
  const { data: agents = [], isLoading, error } = useCompanyAgents(apiBase, companyId);

  const rows = agents.filter((a) => a.status !== "terminated").map(toPcWorkforceAgent);

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

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-lg font-semibold tracking-tight text-foreground">Agents</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Each row is a link to that agent&apos;s <strong>Workspace</strong> (instruction files under{" "}
          <span className="font-mono text-xs">hsmii_home/agents/&lt;name&gt;/</span>). Needs{" "}
          <span className="font-mono text-xs">hsmii_home</span> on the company and{" "}
          <span className="font-mono text-xs">hsm_console</span> for the file API.
        </p>
      </div>

      <Card className="border-admin-border bg-card">
        <CardHeader className="pb-2">
          <CardTitle className="text-base">Workforce</CardTitle>
          <CardDescription>{isLoading ? "Loading…" : `${rows.length} agents`}</CardDescription>
        </CardHeader>
        <CardContent className="p-0">
          <div className="max-h-[min(70vh,560px)] overflow-auto">
            <div className="min-w-[min(100%,720px)] sm:min-w-[640px]">
              <div className="grid grid-cols-[1.2fr_1fr_1fr_100px_100px_72px] gap-2 border-b border-admin-border bg-muted/30 px-4 py-2 font-mono text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
                <span>Name</span>
                <span>Role</span>
                <span>Title</span>
                <span>Status</span>
                <span>Budget/mo</span>
                <span className="text-right"> </span>
              </div>
              {isLoading
                ? Array.from({ length: 6 }).map((_, i) => (
                    <div key={i} className="flex gap-2 border-b border-admin-border px-4 py-3">
                      <Skeleton className="h-4 flex-1" />
                      <Skeleton className="h-4 w-24" />
                    </div>
                  ))
                : rows.length === 0 ? (
                    <p className="px-4 py-6 text-sm text-muted-foreground">
                      No active agents in this company. Import a Paperclip pack or add roster rows in Postgres.
                    </p>
                  )
                : rows.map((a) => {
                    const href = `/workspace/agents/${a.id}?tab=workspace`;
                    return (
                      <Link
                        key={a.id}
                        href={href}
                        prefetch
                        className="pc-table-row grid w-full grid-cols-[1.2fr_1fr_1fr_100px_100px_72px] gap-2 text-left no-underline outline-none ring-offset-background transition-colors hover:bg-white/5 focus-visible:ring-2 focus-visible:ring-ring"
                        onClick={() =>
                          setPropertiesSelection({ kind: "agent", id: a.id, name: a.name })
                        }
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
                        <span className="flex items-center justify-end gap-0.5 font-mono text-[11px] text-muted-foreground">
                          Open
                          <ChevronRight className="h-3 w-3 shrink-0" aria-hidden />
                        </span>
                      </Link>
                    );
                  })}
            </div>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
