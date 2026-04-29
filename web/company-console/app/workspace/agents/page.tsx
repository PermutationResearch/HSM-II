"use client";

import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import {
  ArrowRight,
  ChevronRight,
  Sparkles,
} from "lucide-react";
import { Avatar, AvatarFallback } from "@/app/components/ui/avatar";
import { Badge } from "@/app/components/ui/badge";
import { Button } from "@/app/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/app/components/ui/card";
import { ScrollArea } from "@/app/components/ui/scroll-area";
import { Skeleton } from "@/app/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/app/components/ui/tabs";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import type { HsmCompanyAgentRow, HsmSkillBankEntry } from "@/app/lib/hsm-api-types";
import { fetchSkillBankEntry, useCompanyAgents, useSkillBank } from "@/app/lib/hsm-queries";

function humanizeAgentName(name: string): string {
  return name
    .replace(/_/g, " ")
    .split(/\s+/)
    .filter(Boolean)
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1).toLowerCase())
    .join(" ");
}

function initials(value: string): string {
  return value
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? "")
    .join("");
}

function budgetLabel(cents?: number | null): string {
  if (cents == null) return "No budget set";
  return `$${(cents / 100).toFixed(0)}/mo budget`;
}

function linkedSkillCount(agent: HsmCompanyAgentRow, skillBank: { connected_skill_refs: Record<string, string[]> }): number {
  const refs = new Set<string>();
  for (const [slug, agents] of Object.entries(skillBank.connected_skill_refs)) {
    if (agents.includes(agent.name)) refs.add(slug);
  }
  return refs.size;
}

function statusVariant(status: string): "default" | "secondary" | "outline" {
  const normalized = status.toLowerCase();
  if (normalized === "active" || normalized === "idle") return "default";
  if (normalized === "paused") return "secondary";
  return "outline";
}

function SkillBankPanel({
  apiBase,
  companyId,
  skillBank,
}: {
  apiBase: string;
  companyId: string;
  skillBank: {
    current_skills: HsmSkillBankEntry[];
    recommended_skills: HsmSkillBankEntry[];
    active_agent_count: number;
  };
}) {
  const [selectedSlug, setSelectedSlug] = useState<string | null>(skillBank.current_skills[0]?.slug ?? null);
  const [selectedSkillBody, setSelectedSkillBody] = useState<string | null>(null);
  const installed = skillBank.current_skills;
  const inUse = installed.filter((skill) => (skill.linked_agent_count ?? 0) > 0);
  const recommended = skillBank.recommended_skills;
  const selectedSkill =
    installed.find((skill) => skill.slug === selectedSlug) ??
    recommended.find((skill) => skill.slug === selectedSlug) ??
    null;
  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      if (!selectedSkill || selectedSkill.body) {
        setSelectedSkillBody(selectedSkill?.body ?? null);
        return;
      }
      try {
        const entry = await fetchSkillBankEntry(apiBase, companyId, selectedSkill.slug);
        if (!cancelled) setSelectedSkillBody(entry.body ?? null);
      } catch {
        if (!cancelled) setSelectedSkillBody(null);
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [apiBase, companyId, selectedSkill]);

  const renderSkillList = (skills: HsmSkillBankEntry[], emptyLabel: string) => (
    <div className="space-y-2">
      {skills.length === 0 ? (
        <p className="rounded-xl border border-dashed border-admin-border px-3 py-4 text-sm text-muted-foreground">
          {emptyLabel}
        </p>
      ) : (
        skills.map((skill) => (
          <button
            key={skill.slug}
            type="button"
            onClick={() => setSelectedSlug(skill.slug)}
            className={`w-full rounded-2xl border px-3 py-3 text-left ${
              selectedSkill?.slug === skill.slug
                ? "border-primary/60 bg-primary/10"
                : "border-admin-border bg-black/10 hover:bg-white/5"
            }`}
          >
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-sm font-medium text-foreground">{skill.name || skill.slug}</span>
              <span className="font-mono text-[10px] text-muted-foreground">/{skill.slug}</span>
              {"linked_agent_count" in skill && skill.linked_agent_count ? (
                <Badge variant="outline" className="text-[9px]">
                  {skill.linked_agent_count} linked
                </Badge>
              ) : null}
              {"company_count" in skill && skill.company_count ? (
                <Badge variant="secondary" className="text-[9px]">
                  {skill.company_count} companies
                </Badge>
              ) : null}
            </div>
            <p className="mt-1 line-clamp-2 text-[11px] text-muted-foreground">{skill.description || "No description yet."}</p>
          </button>
        ))
      )}
    </div>
  );

  return (
    <Card className="border-admin-border bg-card/80">
      <CardHeader className="pb-3">
        <div className="flex flex-wrap items-center gap-2">
          <CardTitle className="text-base">Skill bank</CardTitle>
          <Badge variant="outline" className="font-mono text-[10px]">
            {installed.length} installed
          </Badge>
          <Badge variant="outline" className="font-mono text-[10px]">
            {skillBank.active_agent_count} active agents
          </Badge>
        </div>
        <CardDescription>
          Browse what this company uses, what agents have linked, and what other companies use that might be useful here.
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4 lg:grid-cols-[1.05fr_0.95fr]">
        <Tabs defaultValue="installed" className="w-full">
          <TabsList className="border-admin-border">
            <TabsTrigger value="installed">Current company</TabsTrigger>
            <TabsTrigger value="in-use">In use</TabsTrigger>
            <TabsTrigger value="recommended">Useful elsewhere</TabsTrigger>
          </TabsList>
          <TabsContent value="installed" className="pt-3">
            <ScrollArea className="h-[min(50vh,460px)] pr-2">
              {renderSkillList(installed, "No imported skills yet. Import a pack or promote a skill first.")}
            </ScrollArea>
          </TabsContent>
          <TabsContent value="in-use" className="pt-3">
            <ScrollArea className="h-[min(50vh,460px)] pr-2">
              {renderSkillList(inUse, "Agents are not linked to any imported skills yet.")}
            </ScrollArea>
          </TabsContent>
          <TabsContent value="recommended" className="pt-3">
            <ScrollArea className="h-[min(50vh,460px)] pr-2">
              {renderSkillList(recommended, "No cross-company recommendations yet.")}
            </ScrollArea>
          </TabsContent>
        </Tabs>

        <div className="rounded-2xl border border-admin-border bg-black/15 p-4">
          {selectedSkill ? (
            <>
              <div className="flex flex-wrap items-center gap-2">
                <Sparkles className="h-4 w-4 text-primary" />
                <p className="text-base font-medium text-foreground">{selectedSkill.name || selectedSkill.slug}</p>
              </div>
              <p className="mt-1 font-mono text-[10px] text-muted-foreground">/{selectedSkill.slug}</p>
              <p className="mt-3 text-sm leading-relaxed text-muted-foreground">
                {selectedSkill.description || "No description yet."}
              </p>
              {selectedSkill.linked_agents?.length ? (
                <div className="mt-4">
                  <p className="text-[11px] font-medium text-foreground/90">Linked agents</p>
                  <div className="mt-2 flex flex-wrap gap-2">
                    {selectedSkill.linked_agents.map((agentName, i) => (
                      <Badge key={`${agentName}-${i}`} variant="outline" className="text-[10px]">
                        {agentName}
                      </Badge>
                    ))}
                  </div>
                </div>
              ) : null}
              {selectedSkill.company_names?.length ? (
                <div className="mt-4">
                  <p className="text-[11px] font-medium text-foreground/90">Seen in other companies</p>
                  <div className="mt-2 flex flex-wrap gap-2">
                    {selectedSkill.company_names.slice(0, 6).map((companyName) => (
                      <Badge key={companyName} variant="secondary" className="text-[10px]">
                        {companyName}
                      </Badge>
                    ))}
                  </div>
                </div>
              ) : null}
              <div className="mt-4 rounded-xl border border-admin-border/80 bg-black/10 p-3">
                <p className="text-[11px] font-medium text-foreground/90">Preview</p>
                <pre className="mt-2 max-h-[280px] overflow-auto whitespace-pre-wrap font-mono text-[11px] leading-relaxed text-muted-foreground">
                  {selectedSkillBody || "This entry is coming from other companies, so only summary metadata is shown here."}
                </pre>
              </div>
            </>
          ) : (
            <p className="text-sm text-muted-foreground">Choose a skill to inspect its details.</p>
          )}
        </div>
      </CardContent>
    </Card>
  );
}

function AgentCard({
  agent,
  skillBank,
  onSelect,
}: {
  agent: HsmCompanyAgentRow;
  skillBank: { connected_skill_refs: Record<string, string[]> };
  onSelect: () => void;
}) {
  const displayName = humanizeAgentName(agent.name);
  const skillCount = linkedSkillCount(agent, skillBank);
  return (
    <Link
      href={`/workspace/agents/${agent.id}?tab=workspace`}
      onClick={onSelect}
      className="group rounded-[26px] border border-admin-border bg-card/80 p-4 no-underline transition-colors hover:border-primary/50 hover:bg-white/[0.035]"
    >
      <div className="flex items-start gap-3">
        <Avatar size="lg" className="ring-1 ring-white/10">
          <AvatarFallback className="bg-primary/12 text-primary">{initials(displayName)}</AvatarFallback>
        </Avatar>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <p className="truncate text-base font-medium text-foreground">{displayName}</p>
            <Badge variant={statusVariant(agent.status)} className="text-[9px] uppercase">
              {agent.status}
            </Badge>
          </div>
          <p className="mt-1 text-sm text-muted-foreground">
            {agent.title || agent.role}
          </p>
          <p className="mt-3 line-clamp-3 text-[13px] leading-relaxed text-muted-foreground">
            {agent.briefing?.trim() || agent.capabilities?.trim() || "No briefing yet. Open this agent to add identity, skills, and workspace instructions."}
          </p>
        </div>
      </div>
      <div className="mt-4 flex flex-wrap items-center justify-between gap-3 border-t border-admin-border/80 pt-3">
        <div className="flex flex-wrap gap-2 text-[11px] text-muted-foreground">
          <span>{budgetLabel(agent.budget_monthly_cents)}</span>
          <span>{skillCount} linked skills</span>
        </div>
        <span className="inline-flex items-center gap-1 text-xs font-medium text-primary">
          Open workspace
          <ArrowRight className="h-3.5 w-3.5 transition-transform group-hover:translate-x-0.5" />
        </span>
      </div>
    </Link>
  );
}

export default function WorkspaceAgentsPage() {
  const { apiBase, companyId, setPropertiesSelection } = useWorkspace();
  const { data: agents = [], isLoading, error } = useCompanyAgents(apiBase, companyId);
  const { data: skillBank } = useSkillBank(apiBase, companyId);
  const [statusFilter, setStatusFilter] = useState<"all" | "active" | "paused">("all");

  const rows = useMemo(
    () =>
      agents
        .filter((agent) => agent.status !== "terminated")
        .filter((agent) => (statusFilter === "all" ? true : agent.status === statusFilter)),
    [agents, statusFilter],
  );

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
    <div className="space-y-6">
      <div className="space-y-2 border-b border-admin-border pb-4">
        <p className="pc-page-eyebrow">Workspace</p>
        <h1 className="pc-page-title">Agents</h1>
        <p className="max-w-3xl text-sm leading-relaxed text-muted-foreground">
          Inspect the current skill bank, then move through the roster as operators would: by person, domain, and workspace state.
        </p>
      </div>

      {skillBank ? <SkillBankPanel apiBase={apiBase} companyId={companyId} skillBank={skillBank} /> : <Skeleton className="h-72 rounded-3xl" />}

      <Card className="border-admin-border bg-card/80">
        <CardHeader className="pb-3">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <CardTitle className="text-base">Roster</CardTitle>
              <CardDescription>{isLoading ? "Loading…" : `${rows.length} visible agents`}</CardDescription>
            </div>
            <div className="flex flex-wrap gap-2">
              {(["all", "active", "paused"] as const).map((filter) => (
                <Button
                  key={filter}
                  size="sm"
                  variant={statusFilter === filter ? "default" : "outline"}
                  className="h-8"
                  onClick={() => setStatusFilter(filter)}
                >
                  {filter}
                </Button>
              ))}
            </div>
          </div>
        </CardHeader>
        <CardContent>
          {isLoading ? (
            <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
              {Array.from({ length: 6 }).map((_, i) => (
                <Skeleton key={i} className="h-48 rounded-[26px]" />
              ))}
            </div>
          ) : rows.length === 0 ? (
            <div className="rounded-2xl border border-dashed border-admin-border px-4 py-6 text-sm text-muted-foreground">
              No agents match this filter. Import a Paperclip pack or add roster rows in Postgres.
            </div>
          ) : (
            <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
              {rows.map((agent) => (
                <AgentCard
                  key={agent.id}
                  agent={agent}
                  skillBank={skillBank ?? { current_skills: [], recommended_skills: [], connected_skill_refs: {}, active_agent_count: 0 }}
                  onSelect={() => setPropertiesSelection({ kind: "agent", id: agent.id, name: agent.name })}
                />
              ))}
            </div>
          )}
          <div className="mt-4 flex items-center justify-end">
            <Link href="/workspace" className="inline-flex items-center gap-1 text-xs text-primary hover:underline">
              Back to workspace
              <ChevronRight className="h-3 w-3" />
            </Link>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
