"use client";

import { useAgentInventory } from "@/app/lib/hsm-queries";
import { Badge } from "@/app/components/ui/badge";
import { Button } from "@/app/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/app/components/ui/card";
import { ScrollArea } from "@/app/components/ui/scroll-area";
import { Skeleton } from "@/app/components/ui/skeleton";

function formatBytes(n: number): string {
  if (!Number.isFinite(n)) return "—";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

type Props = {
  apiBase: string;
  companyId: string;
  agentId: string;
  agentPackName: string;
  onGoWorkspace: () => void;
};

export function AgentInstructionsSkillsPanel({
  apiBase,
  companyId,
  agentId,
  agentPackName,
  onGoWorkspace,
}: Props) {
  const { data, isLoading, error, refetch, isFetching } = useAgentInventory(apiBase, companyId, agentId);

  if (isLoading) {
    return (
      <div className="space-y-3">
        <Skeleton className="h-8 w-full max-w-md" />
        <Skeleton className="h-40 w-full" />
        <Skeleton className="h-48 w-full" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-4 text-sm text-destructive">
        {error instanceof Error ? error.message : String(error)}
      </div>
    );
  }

  if (!data) {
    return <p className="text-sm text-muted-foreground">No inventory data.</p>;
  }

  const inv = data;

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-sm text-muted-foreground">
          Roster + imported <span className="font-mono text-xs">company_skills</span> + Markdown under{" "}
          <span className="font-mono text-xs text-foreground">agents/{agentPackName}/</span>. Shared libraries
          (e.g.{" "}
          <a
            href="https://github.com/NousResearch/hermes-agent/tree/main/skills"
            className="text-primary underline-offset-4 hover:underline"
            target="_blank"
            rel="noreferrer"
          >
            hermes-agent/skills
          </a>
          ) are merged into <span className="font-mono text-xs">company_skills</span> on pack import when{" "}
          <span className="font-mono text-xs">HSM_SKILL_EXTERNAL_DIRS</span> is set on <span className="font-mono text-xs">hsm_console</span>
          — then any agent can list those slugs in <span className="font-mono text-xs">AGENTS.md</span>.
        </p>
        <div className="flex flex-wrap gap-2">
          <Button type="button" size="sm" variant="outline" onClick={() => void refetch()} disabled={isFetching}>
            {isFetching ? "Refreshing…" : "Refresh"}
          </Button>
          <Button type="button" size="sm" variant="secondary" onClick={onGoWorkspace}>
            Open workspace editor
          </Button>
        </div>
      </div>

      {!inv.hsmii_home_configured ? (
        <p className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-200">
          Company <span className="font-mono text-xs">hsmii_home</span> is not set — instruction file scan will be
          empty. Set it on the company record and run pack import if needed.
        </p>
      ) : null}

      <Card className="border-admin-border">
        <CardHeader className="pb-2">
          <CardTitle className="text-base">AGENTS.md roster · skill refs</CardTitle>
          <CardDescription>
            From <span className="font-mono">adapter_config.paperclip.skills</span> and{" "}
            <span className="font-mono">capabilities</span> (comma-separated).
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {inv.roster_skill_refs.length === 0 ? (
            <p className="text-sm text-muted-foreground">No skill slugs on this roster row.</p>
          ) : (
            <div className="flex flex-wrap gap-1.5">
              {inv.roster_skill_refs.map((s) => (
                <Badge key={s} variant="secondary" className="font-mono text-[10px]">
                  {s}
                </Badge>
              ))}
            </div>
          )}
          {inv.unresolved_skill_refs.length > 0 ? (
            <div className="rounded-md border border-amber-500/25 bg-amber-500/5 px-3 py-2 text-sm">
              <span className="font-medium text-amber-200">Not found in company_skills</span>
              <p className="mt-1 font-mono text-[11px] text-muted-foreground">
                {inv.unresolved_skill_refs.join(", ")}
              </p>
            </div>
          ) : null}
        </CardContent>
      </Card>

      <Card className="border-admin-border">
        <CardHeader className="pb-2">
          <CardTitle className="text-base">Resolved skills (this agent)</CardTitle>
          <CardDescription>Roster entries that match a row in Postgres `company_skills` (by slug).</CardDescription>
        </CardHeader>
        <CardContent>
          {inv.skills_linked.length === 0 ? (
            <p className="text-sm text-muted-foreground">No matches — import the pack or fix slugs in AGENTS.md.</p>
          ) : (
            <ScrollArea className="h-[min(240px,40vh)]">
              <ul className="space-y-3 pr-3">
                {inv.skills_linked.map((x) => (
                  <li
                    key={`${x.ref}-${x.skill.id}`}
                    className="rounded-md border border-admin-border/80 bg-muted/20 p-3 text-sm"
                  >
                    <p className="font-mono text-[11px] text-muted-foreground">
                      ref <span className="text-foreground">{x.ref}</span> →{" "}
                      <span className="text-foreground">{x.skill.slug}</span>
                    </p>
                    <p className="mt-1 font-medium text-foreground">{x.skill.name || x.skill.slug}</p>
                    {x.skill.description ? (
                      <p className="mt-1 text-xs text-muted-foreground line-clamp-3">{x.skill.description}</p>
                    ) : null}
                    <p className="mt-1 font-mono text-[10px] text-muted-foreground">{x.skill.skill_path}</p>
                  </li>
                ))}
              </ul>
            </ScrollArea>
          )}
        </CardContent>
      </Card>

      <Card className="border-admin-border">
        <CardHeader className="pb-2">
          <CardTitle className="text-base">Instruction Markdown files</CardTitle>
          <CardDescription>
            Scanned up to 4 levels under <span className="font-mono">agents/{agentPackName}/</span> (max 200
            files).
          </CardDescription>
        </CardHeader>
        <CardContent>
          {inv.instruction_markdown_files.length === 0 ? (
            <p className="text-sm text-muted-foreground">No .md files found on disk for this agent folder.</p>
          ) : (
            <div className="rounded-md border border-admin-border">
              <div className="grid grid-cols-[1fr_88px] gap-2 border-b border-admin-border bg-muted/30 px-3 py-2 font-mono text-[10px] font-semibold uppercase tracking-wide text-muted-foreground sm:grid-cols-[1fr_88px_120px]">
                <span>Path</span>
                <span className="text-right">Size</span>
                <span className="hidden sm:block">Updated</span>
              </div>
              <ScrollArea className="h-[min(280px,36vh)]">
                <ul>
                  {inv.instruction_markdown_files.map((f) => (
                    <li
                      key={f.path}
                      className="grid grid-cols-[1fr_88px] gap-2 border-b border-admin-border/60 px-3 py-2 text-sm last:border-b-0 sm:grid-cols-[1fr_88px_120px]"
                    >
                      <span className="break-all font-mono text-[11px] text-foreground">{f.path}</span>
                      <span className="text-right font-mono text-[11px] text-muted-foreground">
                        {formatBytes(f.size_bytes)}
                      </span>
                      <span className="hidden font-mono text-[10px] text-muted-foreground sm:block">
                        {f.modified_at
                          ? new Date(f.modified_at).toLocaleString(undefined, {
                              dateStyle: "short",
                              timeStyle: "short",
                            })
                          : "—"}
                      </span>
                    </li>
                  ))}
                </ul>
              </ScrollArea>
            </div>
          )}
        </CardContent>
      </Card>

      <Card className="border-admin-border">
        <CardHeader className="pb-2">
          <CardTitle className="text-base">Full company skill catalog</CardTitle>
          <CardDescription>
            Every <span className="font-mono">company_skills</span> row for this workspace. Highlighted = slug on this
            agent&apos;s roster.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {inv.company_skills_catalog.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              No imported skills — run <span className="font-mono text-xs">POST …/import-paperclip-home</span> or
              equivalent.
            </p>
          ) : (
            <ScrollArea className="h-[min(360px,50vh)]">
              <div className="min-w-[min(100%,640px)] space-y-1 pr-3">
                {inv.company_skills_catalog.map((s) => (
                  <div
                    key={s.id}
                    className={`flex flex-col gap-1 rounded-md border px-3 py-2 sm:flex-row sm:items-start sm:justify-between ${
                      s.on_agent_roster
                        ? "border-emerald-500/35 bg-emerald-500/10"
                        : "border-admin-border/80 bg-muted/10"
                    }`}
                  >
                    <div className="min-w-0">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="font-medium text-foreground">{s.name || s.slug}</span>
                        {s.on_agent_roster ? (
                          <Badge className="text-[9px] uppercase">On this agent</Badge>
                        ) : null}
                      </div>
                      <p className="font-mono text-[10px] text-muted-foreground">{s.slug}</p>
                      {s.description ? (
                        <p className="mt-1 text-xs text-muted-foreground line-clamp-2">{s.description}</p>
                      ) : null}
                      <p className="mt-1 font-mono text-[10px] text-muted-foreground/80">{s.skill_path}</p>
                    </div>
                  </div>
                ))}
              </div>
            </ScrollArea>
          )}
        </CardContent>
      </Card>

      {inv.agent.briefing_preview ? (
        <Card className="border-admin-border">
          <CardHeader className="pb-2">
            <CardTitle className="text-base">Briefing preview</CardTitle>
            <CardDescription>First ~480 chars from roster <span className="font-mono">briefing</span>.</CardDescription>
          </CardHeader>
          <CardContent>
            <pre className="max-h-[200px] overflow-auto whitespace-pre-wrap rounded-md border border-admin-border bg-muted/20 p-3 font-mono text-[11px] text-muted-foreground">
              {inv.agent.briefing_preview}
            </pre>
          </CardContent>
        </Card>
      ) : null}

      <details className="rounded-md border border-admin-border bg-muted/10 px-3 py-2 text-sm">
        <summary className="cursor-pointer font-medium text-foreground">Raw adapter_config (JSON)</summary>
        <pre className="mt-2 max-h-[240px] overflow-auto font-mono text-[10px] text-muted-foreground">
          {JSON.stringify(inv.agent.adapter_config ?? {}, null, 2)}
        </pre>
      </details>
    </div>
  );
}
