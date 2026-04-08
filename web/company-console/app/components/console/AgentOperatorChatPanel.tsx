"use client";

import { useCallback, useState } from "react";
import Link from "next/link";
import { Check, Copy, MessageSquare } from "lucide-react";
import { Badge } from "@/app/components/ui/badge";
import { Button } from "@/app/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/app/components/ui/card";
import { ScrollArea } from "@/app/components/ui/scroll-area";
import { Skeleton } from "@/app/components/ui/skeleton";
import { useAgentOperatorThread } from "@/app/lib/hsm-queries";
import { cn } from "@/app/lib/utils";

type Props = {
  apiBase: string;
  companyId: string;
  agentId: string;
  agentDisplayName: string;
};

export function AgentOperatorChatPanel({ apiBase, companyId, agentId, agentDisplayName }: Props) {
  const { data, isLoading, error, refetch, isFetching } = useAgentOperatorThread(apiBase, companyId, agentId);
  const [copied, setCopied] = useState(false);

  const conversationKey = `${companyId}:${agentId}`;

  const copyId = useCallback(async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      /* ignore */
    }
  }, []);

  if (isLoading) {
    return (
      <div className="space-y-4">
        <Skeleton className="h-24 w-full rounded-lg" />
        <Skeleton className="h-48 w-full rounded-lg" />
      </div>
    );
  }

  if (error) {
    return (
      <Card className="border-admin-border">
        <CardHeader>
          <CardTitle className="text-base">Operator chat &amp; thread</CardTitle>
        </CardHeader>
        <CardContent className="text-sm text-destructive">
          {error instanceof Error ? error.message : String(error)}
        </CardContent>
      </Card>
    );
  }

  const digest = data?.compact_digest?.trim() ?? "";
  const totalTasks = data?.total_tasks ?? 0;
  const flatLen = data?.notes_flat?.length ?? 0;

  return (
    <div className="space-y-6">
      <Card className="border-admin-border">
        <CardHeader className="space-y-2">
          <div className="flex flex-wrap items-center gap-2">
            <MessageSquare className="size-4 text-muted-foreground" aria-hidden />
            <CardTitle className="text-base">Operator messaging</CardTitle>
          </div>
          <CardDescription className="leading-relaxed">
            Chat messages from the right rail are appended to task{" "}
            <span className="font-mono text-[11px] text-foreground/90">context_notes</span> (stigmergic
            handoffs). This page shows the stable IDs and a server-built compact digest for LLM / handoff.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4 text-sm">
          <div className="rounded-lg border border-border/80 bg-muted/30 p-4">
            <p className="font-mono text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
              Conversation anchor
            </p>
            <div className="mt-2 flex flex-wrap items-center gap-2">
              <span className="break-all font-mono text-xs text-foreground" title="Workforce agent UUID">
                {agentId}
              </span>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-8 gap-1.5 font-mono text-[10px]"
                onClick={() => copyId(agentId)}
              >
                {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
                {copied ? "Copied" : "Copy agent id"}
              </Button>
            </div>
            <p className="mt-3 font-mono text-[10px] text-muted-foreground">
              Composite key (company:agent):{" "}
              <button
                type="button"
                className="break-all text-left text-foreground/80 hover:underline"
                onClick={() => copyId(conversationKey)}
                title="Copy"
              >
                {conversationKey}
              </button>
            </p>
          </div>

          <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
            <Badge variant="secondary" className="font-mono text-[10px]">
              {totalTasks} task{totalTasks === 1 ? "" : "s"} with thread data
            </Badge>
            {flatLen > 0 ? (
              <Badge variant="outline" className="font-mono text-[10px]">
                {flatLen} note{flatLen === 1 ? "" : "s"}
              </Badge>
            ) : null}
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-7 font-mono text-[10px]"
              disabled={isFetching}
              onClick={() => void refetch()}
            >
              {isFetching ? "Refreshing…" : "Refresh digest"}
            </Button>
          </div>

          <p className="text-muted-foreground">
            To send a new message, use the{" "}
            <span className="text-foreground/90">right-hand rail</span> (header toggle), select{" "}
            <span className="font-medium text-foreground">{agentDisplayName}</span>, and type in the thread.
          </p>
        </CardContent>
      </Card>

      <Card className={cn("border-admin-border", !digest && "border-dashed")}>
        <CardHeader className="space-y-1 pb-2">
          <CardTitle className="text-base">Compact context (server)</CardTitle>
          <CardDescription>
            Rolled-up operator notes across tasks — same field the API uses for handoffs and LLM context.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {digest ? (
            <ScrollArea className="max-h-[min(55vh,480px)] rounded-md border border-border/60 bg-background/50">
              <pre className="whitespace-pre-wrap break-words p-4 font-mono text-[11px] leading-relaxed text-foreground/95">
                {digest}
              </pre>
            </ScrollArea>
          ) : (
            <p className="text-sm leading-relaxed text-muted-foreground">
              No operator notes yet. Open the right rail, pick this agent, and send a message (or assign a task
              and add stigmergic notes). Then refresh this digest.
            </p>
          )}
          <div className="mt-4 flex flex-wrap gap-2">
            <Button variant="outline" size="sm" asChild>
              <Link href="/workspace/issues">Open issues</Link>
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
