"use client";

import Link from "next/link";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/app/components/ui/card";
import { ScrollArea } from "@/app/components/ui/scroll-area";
import { Skeleton } from "@/app/components/ui/skeleton";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { hsmSpendRowToSpendSummary } from "@/app/lib/paperclip-hsm-bridge";
import { useCompanyAgents, useCompanySpendSummary } from "@/app/lib/hsm-queries";

const usd = new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 4 });

export default function WorkspaceCostsPage() {
  const { apiBase, companyId, companies } = useWorkspace();
  const company = companies.find((c) => c.id === companyId);
  const { data: spend, isLoading: spendLoading, error: spendError } = useCompanySpendSummary(apiBase, companyId);
  const { data: agents = [], isLoading: agentsLoading } = useCompanyAgents(apiBase, companyId);

  if (!companyId) {
    return <p className="pc-page-desc">Select a company in the header.</p>;
  }

  const budgetCents = agents
    .filter((a) => a.status !== "terminated" && typeof a.budget_monthly_cents === "number")
    .reduce((s, a) => s + Math.max(0, a.budget_monthly_cents ?? 0), 0);
  const budgetUsd = budgetCents / 100;
  const total = spend?.total_usd ?? 0;
  const utilPct = budgetUsd > 0 ? Math.min(100, Math.round((total / budgetUsd) * 100)) : null;

  return (
    <div className="space-y-4">
      <div>
        <p className="pc-page-eyebrow">Finance</p>
        <h1 className="pc-page-title">Costs</h1>
        <p className="pc-page-desc">
          Rollup from <span className="font-mono text-xs">GET …/spend/summary</span>
          {company ? (
            <>
              {" "}
              · <span className="text-foreground">{company.display_name}</span>
            </>
          ) : null}
          . Compare to workforce monthly budgets on{" "}
          <Link href="/workspace/agents" className="text-primary underline-offset-4 hover:underline">
            Agents
          </Link>
          .
        </p>
      </div>

      {spendError ? (
        <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-4 text-sm">
          {spendError instanceof Error ? spendError.message : String(spendError)}
        </div>
      ) : null}

      <div className="grid gap-4 md:grid-cols-2">
        <Card className="pc-panel border-admin-border">
          <CardHeader className="pb-2">
            <CardTitle className="text-base">Month spend (recorded)</CardTitle>
            <CardDescription>
              {spendLoading ? "Loading…" : `Total ${usd.format(total)} USD`}
            </CardDescription>
          </CardHeader>
          <CardContent>
            {spendLoading ? (
              <Skeleton className="h-10 w-40" />
            ) : (
              <p className="font-mono text-2xl tabular-nums text-foreground">{usd.format(total)}</p>
            )}
            {utilPct != null && !agentsLoading ? (
              <p className="mt-2 text-sm text-muted-foreground">
                ~{utilPct}% of {usd.format(budgetUsd)} combined monthly agent budgets (if set).
              </p>
            ) : !agentsLoading && budgetUsd <= 0 ? (
              <p className="mt-2 text-sm text-muted-foreground">No monthly budgets on agents — set budgets to see utilization.</p>
            ) : null}
          </CardContent>
        </Card>

        <Card className="pc-panel border-admin-border">
          <CardHeader className="pb-2">
            <CardTitle className="text-base">Paperclip-shaped view</CardTitle>
            <CardDescription>Optional camelCase adapter for downstream charts</CardDescription>
          </CardHeader>
          <CardContent className="font-mono text-xs text-muted-foreground">
            {spend ? (
              <pre className="max-h-40 overflow-auto rounded border border-admin-border bg-black/30 p-2 text-[11px]">
                {JSON.stringify(hsmSpendRowToSpendSummary(spend), null, 2)}
              </pre>
            ) : spendLoading ? (
              <Skeleton className="h-24 w-full" />
            ) : (
              "—"
            )}
          </CardContent>
        </Card>
      </div>

      <Card className="pc-panel border-admin-border">
        <CardHeader className="pb-2">
          <CardTitle className="text-base">By kind</CardTitle>
          <CardDescription>Grouped spend kinds from Company OS</CardDescription>
        </CardHeader>
        <CardContent className="p-0">
          <ScrollArea className="h-[min(50vh,400px)]">
            <div className="min-w-[480px]">
              <div className="pc-table-header grid-cols-[1fr_140px]">
                <span>Kind</span>
                <span>Amount (USD)</span>
              </div>
              {spendLoading
                ? Array.from({ length: 5 }).map((_, i) => (
                    <div key={i} className="flex gap-2 border-b border-admin-border px-4 py-3">
                      <Skeleton className="h-4 flex-1" />
                      <Skeleton className="h-4 w-24" />
                    </div>
                  ))
                : (spend?.by_kind ?? []).length === 0
                  ? (
                      <p className="px-4 py-6 text-sm text-muted-foreground">No spend events yet for this company.</p>
                    )
                  : (spend?.by_kind ?? []).map((row) => (
                      <div
                        key={row.kind}
                        className="pc-table-row grid grid-cols-[1fr_140px] gap-2"
                      >
                        <span className="font-mono text-xs text-foreground">{row.kind}</span>
                        <span className="font-mono text-xs tabular-nums text-muted-foreground">
                          {usd.format(row.amount_usd)}
                        </span>
                      </div>
                    ))}
            </div>
          </ScrollArea>
        </CardContent>
      </Card>
    </div>
  );
}
