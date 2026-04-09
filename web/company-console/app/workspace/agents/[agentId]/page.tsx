"use client";

import { Suspense, useEffect, useMemo } from "react";
import Link from "next/link";
import { useParams, useRouter, useSearchParams } from "next/navigation";
import { MessageSquare, Pause, Play } from "lucide-react";
import { AgentInstructionsSkillsPanel } from "@/app/components/console/AgentInstructionsSkillsPanel";
import { AgentScopedMemoryPanel } from "@/app/components/console/AgentScopedMemoryPanel";
import { AgentOperatorChatPanel } from "@/app/components/console/AgentOperatorChatPanel";
import { AgentWorkspacePanel } from "@/app/components/console/AgentWorkspacePanel";
import { AgentRunsPanel } from "@/app/components/console/AgentRunsPanel";
import { Badge } from "@/app/components/ui/badge";
import { Button } from "@/app/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/app/components/ui/card";
import { Skeleton } from "@/app/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/app/components/ui/tabs";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { useCompanyAgents } from "@/app/lib/hsm-queries";

const UUID_RE =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

const TAB_VALUES = [
  "workspace",
  "memory",
  "instructions",
  "dashboard",
  "chat",
  "configuration",
  "runs",
] as const;
type AgentTab = (typeof TAB_VALUES)[number];

function humanizeAgentName(name: string): string {
  return name
    .replace(/_/g, " ")
    .split(/\s+/)
    .filter(Boolean)
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1).toLowerCase())
    .join(" ");
}

function AgentDetailFallback() {
  return (
    <div className="space-y-4">
      <Skeleton className="h-10 w-64" />
      <Skeleton className="h-10 w-full max-w-md" />
      <Skeleton className="h-96 w-full rounded-lg" />
    </div>
  );
}

function AgentDetailContent() {
  const params = useParams();
  const searchParams = useSearchParams();
  const router = useRouter();
  const agentId = typeof params.agentId === "string" ? params.agentId : "";
  const { apiBase, companyId, companies, setPropertiesSelection, postgresOk } = useWorkspace();
  const { data: agents = [], isLoading, error } = useCompanyAgents(apiBase, companyId);

  const validId = useMemo(() => UUID_RE.test(agentId), [agentId]);
  const agent = useMemo(() => agents.find((a) => a.id === agentId), [agents, agentId]);

  const rawTab = searchParams.get("tab");
  const tab: AgentTab =
    rawTab && (TAB_VALUES as readonly string[]).includes(rawTab) ? (rawTab as AgentTab) : "workspace";

  const setTab = (next: string) => {
    if (!(TAB_VALUES as readonly string[]).includes(next)) return;
    router.replace(`/workspace/agents/${agentId}?tab=${next}`, { scroll: false });
  };

  useEffect(() => {
    if (!agent) return;
    setPropertiesSelection({
      kind: "agent",
      id: agent.id,
      name: agent.name,
    });
  }, [agent, setPropertiesSelection]);

  if (!companyId) {
    return <p className="pc-page-desc">Select a company in the header.</p>;
  }

  if (!validId) {
    return (
      <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-4 text-sm">
        Invalid agent id.
      </div>
    );
  }

  if (error) {
    return (
      <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-4 text-sm">
        {error instanceof Error ? error.message : String(error)}
      </div>
    );
  }

  if (isLoading || !agent) {
    return <AgentDetailFallback />;
  }

  const displayName = humanizeAgentName(agent.name);
  const issueKeyPrefix = (companies.find((c) => c.id === companyId)?.issue_key_prefix ?? "HSM").toUpperCase();

  const statusTone =
    agent.status === "active" || agent.status === "idle"
      ? "secondary"
      : agent.status === "terminated"
        ? "outline"
        : "outline";

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-4 border-b border-admin-border pb-4 sm:flex-row sm:items-start sm:justify-between">
        <div className="min-w-0 space-y-1">
          <div className="flex flex-wrap items-center gap-2">
            <h1 className="text-xl font-semibold tracking-tight text-foreground">{displayName}</h1>
            <MessageSquare className="h-4 w-4 text-muted-foreground" aria-hidden />
            <Badge variant={statusTone} className="font-mono text-[10px] uppercase">
              {agent.status}
            </Badge>
          </div>
          <p className="text-sm text-muted-foreground">
            <span className="font-medium text-foreground/80">{agent.role}</span>
            {agent.title ? (
              <>
                {" "}
                — {agent.title}
              </>
            ) : null}
          </p>
          <p className="font-mono text-[10px] text-muted-foreground">
            Pack folder <span className="text-foreground">{agent.name}</span> · id {agent.id}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button type="button" size="sm" variant="secondary" asChild>
            <Link href="/workspace/issues">Assign task</Link>
          </Button>
          <Button type="button" size="sm" variant="outline" disabled title="Not wired in company-console">
            <Play className="mr-1 h-3.5 w-3.5" />
            Run heartbeat
          </Button>
          <Button type="button" size="sm" variant="outline" disabled title="Not wired in company-console">
            <Pause className="mr-1 h-3.5 w-3.5" />
            Pause
          </Button>
          <Button type="button" size="sm" variant="outline" disabled title="Use the workspace right rail">
            Chat
          </Button>
        </div>
      </div>

      <Tabs value={tab} onValueChange={setTab} className="w-full">
        <TabsList variant="line" className="w-full min-w-0 flex-wrap justify-start sm:w-auto">
          <TabsTrigger value="workspace">Workspace</TabsTrigger>
          <TabsTrigger value="memory">Memory</TabsTrigger>
          <TabsTrigger value="instructions">Instructions &amp; skills</TabsTrigger>
          <TabsTrigger value="dashboard">Dashboard</TabsTrigger>
          <TabsTrigger value="chat">Chat</TabsTrigger>
          <TabsTrigger value="configuration">Configuration</TabsTrigger>
          <TabsTrigger value="runs">Runs</TabsTrigger>
        </TabsList>

        <TabsContent value="workspace" className="mt-6 scroll-mt-2 pt-2 sm:mt-8 sm:pt-3">
          <AgentWorkspacePanel
            apiBase={apiBase}
            companyId={companyId}
            agentPackName={agent.name}
            assigneeDisplayName={displayName}
            issueKeyPrefix={issueKeyPrefix}
          />
        </TabsContent>

        <TabsContent value="memory" className="mt-4">
          <AgentScopedMemoryPanel
            apiBase={apiBase}
            companyId={companyId}
            agentId={agent.id}
            postgresOk={postgresOk}
          />
        </TabsContent>

        <TabsContent value="instructions" className="mt-4">
          <AgentInstructionsSkillsPanel
            apiBase={apiBase}
            companyId={companyId}
            agentId={agent.id}
            agentPackName={agent.name}
            onGoWorkspace={() => setTab("workspace")}
          />
        </TabsContent>

        <TabsContent value="dashboard" className="mt-4">
          <Card className="border-admin-border">
            <CardHeader>
              <CardTitle className="text-base">Dashboard</CardTitle>
              <CardDescription>Overview for this workforce agent.</CardDescription>
            </CardHeader>
            <CardContent className="space-y-3 text-sm text-muted-foreground">
              <p>
                See the{" "}
                <button
                  type="button"
                  className="text-primary underline-offset-4 hover:underline"
                  onClick={() => setTab("memory")}
                >
                  Memory
                </button>{" "}
                tab for Paperclip-style agent-scoped Postgres memory, the{" "}
                <button
                  type="button"
                  className="text-primary underline-offset-4 hover:underline"
                  onClick={() => setTab("instructions")}
                >
                  Instructions &amp; skills
                </button>{" "}
                tab for roster skill refs and Markdown, and{" "}
                <button
                  type="button"
                  className="text-primary underline-offset-4 hover:underline"
                  onClick={() => setTab("workspace")}
                >
                  Workspace
                </button>{" "}
                for files.
              </p>
              <Button variant="outline" size="sm" asChild>
                <Link href="/workspace/issues">Open issues</Link>
              </Button>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="chat" className="mt-6 scroll-mt-2 pt-2 sm:mt-8 sm:pt-3">
          <AgentOperatorChatPanel
            apiBase={apiBase}
            companyId={companyId}
            agentId={agent.id}
            agentDisplayName={displayName}
          />
        </TabsContent>

        <TabsContent value="configuration" className="mt-4">
          <Card className="border-admin-border">
            <CardHeader>
              <CardTitle className="text-base">Configuration</CardTitle>
              <CardDescription>Roster fields from Postgres.</CardDescription>
            </CardHeader>
            <CardContent className="space-y-2 font-mono text-xs text-muted-foreground">
              <p>
                <span className="text-foreground">name</span> {agent.name}
              </p>
              <p>
                <span className="text-foreground">role</span> {agent.role}
              </p>
              <p>
                <span className="text-foreground">title</span> {agent.title ?? "—"}
              </p>
              <p>
                <span className="text-foreground">status</span> {agent.status}
              </p>
              <p>
                <span className="text-foreground">budget_monthly_cents</span>{" "}
                {agent.budget_monthly_cents ?? "—"}
              </p>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="runs" className="mt-4">
          <AgentRunsPanel
            apiBase={apiBase}
            companyId={companyId}
            agentId={agent.id}
          />
        </TabsContent>
      </Tabs>
    </div>
  );
}

export default function AgentDetailPage() {
  return (
    <Suspense fallback={<AgentDetailFallback />}>
      <AgentDetailContent />
    </Suspense>
  );
}
