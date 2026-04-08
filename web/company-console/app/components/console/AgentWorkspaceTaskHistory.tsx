"use client";

import Link from "next/link";
import { useMemo } from "react";
import { ExternalLink } from "lucide-react";
import { Badge } from "@/app/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/app/components/ui/card";
import { ScrollArea } from "@/app/components/ui/scroll-area";
import { Skeleton } from "@/app/components/ui/skeleton";
import { taskToPcIssue } from "@/app/lib/hsm-api-adapter";
import type { HsmTaskRow } from "@/app/lib/hsm-api-types";
import { useCompanyTasks } from "@/app/lib/hsm-queries";

function personaMatches(a: string | null | undefined, b: string): boolean {
  return (a ?? "").trim().toLowerCase() === b.trim().toLowerCase();
}

/** Tasks owned by this persona or currently checked out to them. */
export function tasksForAgentPersona(tasks: HsmTaskRow[], agentPersona: string): HsmTaskRow[] {
  const p = agentPersona.trim().toLowerCase();
  if (!p) return [];
  return tasks.filter(
    (t) => personaMatches(t.owner_persona, p) || personaMatches(t.checked_out_by, p),
  );
}

function sortAgentTasks(a: HsmTaskRow, b: HsmTaskRow): number {
  const pr = (b.priority ?? 0) - (a.priority ?? 0);
  if (pr !== 0) return pr;
  const da = typeof a.display_number === "number" ? a.display_number : 0;
  const db = typeof b.display_number === "number" ? b.display_number : 0;
  return db - da;
}

type Props = {
  apiBase: string;
  companyId: string;
  /** Roster `company_agents.name` — matches `owner_persona` / `checked_out_by` on tasks. */
  agentPersonaName: string;
  issueKeyPrefix: string;
};

export function AgentWorkspaceTaskHistory({ apiBase, companyId, agentPersonaName, issueKeyPrefix }: Props) {
  const { data: tasks = [], isLoading, error } = useCompanyTasks(apiBase, companyId);

  const rows = useMemo(() => {
    const filtered = tasksForAgentPersona(tasks, agentPersonaName);
    return [...filtered].sort(sortAgentTasks);
  }, [tasks, agentPersonaName]);

  const prefix = issueKeyPrefix.toUpperCase();

  if (error) {
    return (
      <Card className="border-admin-border">
        <CardHeader className="space-y-2 pb-4">
          <CardTitle className="text-base">Tasks for this agent</CardTitle>
        </CardHeader>
        <CardContent className="text-sm text-destructive">
          {error instanceof Error ? error.message : String(error)}
        </CardContent>
      </Card>
    );
  }

  return (
    <Card className="border-admin-border gap-0 py-5">
      <CardHeader className="space-y-3 pb-0">
        <CardTitle className="text-base">Tasks for this agent</CardTitle>
        <CardDescription className="leading-relaxed">
          Issue ids and status where this agent is <span className="text-foreground/90">owner</span> or has{" "}
          <span className="text-foreground/90">checkout</span>. Open a row in Issues for full detail and runs.
        </CardDescription>
      </CardHeader>
      <CardContent className="mt-6 border-t border-border/60 p-0 pt-6">
        {isLoading ? (
          <div className="space-y-2 px-6 pb-4">
            <Skeleton className="h-8 w-full" />
            <Skeleton className="h-8 w-full" />
            <Skeleton className="h-8 w-full" />
          </div>
        ) : rows.length === 0 ? (
          <p className="px-6 pb-1 text-sm leading-relaxed text-muted-foreground">
            No tasks yet. Assign work from{" "}
            <Link href="/workspace/issues" className="text-primary underline-offset-4 hover:underline">
              Issues
            </Link>{" "}
            with this agent as owner, or check out a task to this persona.
          </p>
        ) : (
          <ScrollArea className="max-h-[min(40vh,320px)] px-1 pb-1">
            <table className="w-full border-collapse text-sm">
              <thead>
                <tr className="border-b border-admin-border text-left font-mono text-[10px] uppercase tracking-wide text-muted-foreground">
                  <th className="px-4 py-3 font-medium">Id</th>
                  <th className="px-2 py-3 font-medium">Title</th>
                  <th className="px-2 py-3 font-medium">State</th>
                  <th className="px-4 py-3 font-medium">Role</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((t) => {
                  const pc = taskToPcIssue(t, prefix);
                  const owner = personaMatches(t.owner_persona, agentPersonaName);
                  const checkout = personaMatches(t.checked_out_by, agentPersonaName);
                  const run = t.run;
                  return (
                    <tr key={t.id} className="border-b border-admin-border/60 hover:bg-muted/30">
                      <td className="px-4 py-3 align-top font-mono text-xs">
                        <Link
                          href={`/workspace/issues?focus=${encodeURIComponent(t.id)}`}
                          className="text-primary underline-offset-4 hover:underline"
                        >
                          {pc.identifier}
                        </Link>
                      </td>
                      <td className="max-w-[min(40vw,280px)] px-2 py-3 align-top">
                        <span className="line-clamp-2 text-foreground/95">{t.title}</span>
                      </td>
                      <td className="whitespace-nowrap px-2 py-3 align-top text-xs text-muted-foreground">
                        <span className="text-foreground/90">{t.state}</span>
                        {run?.status ? (
                          <Badge variant="outline" className="ml-1 font-mono text-[9px]">
                            run {run.status}
                          </Badge>
                        ) : null}
                      </td>
                      <td className="px-4 py-3 align-top">
                        <div className="flex flex-wrap gap-1">
                          {owner ? (
                            <Badge variant="secondary" className="font-mono text-[9px]">
                              owner
                            </Badge>
                          ) : null}
                          {checkout ? (
                            <Badge variant="outline" className="font-mono text-[9px]">
                              checkout
                            </Badge>
                          ) : null}
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </ScrollArea>
        )}
        {!isLoading && rows.length > 0 ? (
          <div className="flex items-center justify-end gap-2 border-t border-admin-border px-4 py-3">
            <Link
              href="/workspace/issues"
              className="inline-flex items-center gap-1 font-mono text-[11px] text-muted-foreground hover:text-foreground"
            >
              All issues
              <ExternalLink className="size-3 opacity-70" aria-hidden />
            </Link>
          </div>
        ) : null}
      </CardContent>
    </Card>
  );
}
