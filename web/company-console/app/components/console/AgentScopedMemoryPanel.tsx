"use client";

import { useCallback, useEffect, useState } from "react";
import { Loader2, Plus, RefreshCw, Trash2 } from "lucide-react";
import type { CompanyMemoryEntry } from "@/app/components/CompanySharedMemoryPanel";
import { Button } from "@/app/components/ui/button";
import { Input } from "@/app/components/ui/input";
import { ScrollArea } from "@/app/components/ui/scroll-area";
import { Textarea } from "@/app/components/ui/textarea";
import { companyOsUrl } from "@/app/lib/company-api-url";

type Props = {
  apiBase: string;
  companyId: string;
  /** `company_agents.id` — scopes memory rows to this roster agent. */
  agentId: string;
  postgresOk: boolean;
};

export function AgentScopedMemoryPanel({ apiBase, companyId, agentId, postgresOk }: Props) {
  const [entries, setEntries] = useState<CompanyMemoryEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchDraft, setSearchDraft] = useState("");
  const [searchApplied, setSearchApplied] = useState("");
  const [newTitle, setNewTitle] = useState("");
  const [newBody, setNewBody] = useState("");
  const [newTags, setNewTags] = useState("");
  const [saving, setSaving] = useState(false);

  const refetch = useCallback(
    async (needle: string) => {
      if (!postgresOk) {
        setEntries([]);
        return;
      }
      setLoading(true);
      setError(null);
      try {
        const sp = new URLSearchParams({ scope: "agent", company_agent_id: agentId });
        const n = needle.trim();
        if (n) sp.set("q", n);
        const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/memory?${sp}`));
        const j = (await r.json().catch(() => ({}))) as { entries?: CompanyMemoryEntry[]; error?: string };
        if (!r.ok) throw new Error(j.error ?? `memory ${r.status}`);
        setEntries(j.entries ?? []);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
        setEntries([]);
      } finally {
        setLoading(false);
      }
    },
    [apiBase, companyId, agentId, postgresOk],
  );

  useEffect(() => {
    void refetch(searchApplied);
  }, [refetch, searchApplied]);

  if (!postgresOk) {
    return (
      <div className="rounded-lg border border-amber-900/40 bg-amber-950/20 px-4 py-3 text-sm text-amber-100/90">
        Agent memory needs Postgres. Set{" "}
        <code className="font-mono text-[11px]">HSM_COMPANY_OS_DATABASE_URL</code> and restart{" "}
        <code className="font-mono text-[11px]">hsm_console</code>.
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <p className="text-sm text-muted-foreground">
        Entries with <span className="font-mono text-xs">scope=agent</span> are merged into this agent&apos;s task
        LLM context (after company shared memory). Same API as Paperclip-style company memory, filtered to this
        roster id.
      </p>

      {error ? <p className="text-sm text-destructive">{error}</p> : null}

      <div className="flex flex-wrap items-end gap-2">
        <div className="min-w-[200px] flex-1 space-y-1">
          <label className="font-mono text-[10px] uppercase tracking-wide text-muted-foreground">Search</label>
          <Input
            placeholder="Title, body, summaries…"
            value={searchDraft}
            onChange={(e) => setSearchDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") setSearchApplied(searchDraft.trim());
            }}
          />
        </div>
        <Button type="button" variant="secondary" size="sm" onClick={() => setSearchApplied(searchDraft.trim())}>
          Search
        </Button>
        <Button type="button" variant="outline" size="sm" disabled={loading} onClick={() => void refetch(searchApplied)}>
          {loading ? <Loader2 className="h-4 w-4 animate-spin" /> : <RefreshCw className="h-4 w-4" />}
          <span className="ml-1.5">Refresh</span>
        </Button>
      </div>

      <div className="rounded-md border border-admin-border p-4">
        <h2 className="mb-1 text-sm font-semibold">Add agent memory</h2>
        <p className="mb-3 text-xs text-muted-foreground">
          Private to this agent on this company — not shown in the workspace shared pool.
        </p>
        <Input
          className="mb-2"
          placeholder="Title"
          value={newTitle}
          onChange={(e) => setNewTitle(e.target.value)}
        />
        <Textarea
          className="mb-2 min-h-[100px] font-mono text-xs"
          placeholder="Body (markdown ok)"
          value={newBody}
          onChange={(e) => setNewBody(e.target.value)}
        />
        <Input
          className="mb-3"
          placeholder="Tags (comma-separated, optional)"
          value={newTags}
          onChange={(e) => setNewTags(e.target.value)}
        />
        <Button
          type="button"
          size="sm"
          disabled={saving || !newTitle.trim()}
          onClick={async () => {
            setError(null);
            setSaving(true);
            try {
              const tags = newTags
                .split(/[,;]/)
                .map((s) => s.trim())
                .filter(Boolean);
              const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/memory`), {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({
                  title: newTitle.trim(),
                  body: newBody.trim(),
                  scope: "agent",
                  company_agent_id: agentId,
                  tags,
                  source: "human",
                  kind: "general",
                }),
              });
              const j = (await r.json().catch(() => ({}))) as { error?: string };
              if (!r.ok) throw new Error(j.error ?? `${r.status}`);
              setNewTitle("");
              setNewBody("");
              setNewTags("");
              await refetch(searchApplied);
            } catch (e) {
              setError(e instanceof Error ? e.message : String(e));
            } finally {
              setSaving(false);
            }
          }}
        >
          {saving ? <Loader2 className="mr-1.5 h-4 w-4 animate-spin" /> : <Plus className="mr-1.5 h-4 w-4" />}
          Save entry
        </Button>
      </div>

      <div>
        <h2 className="mb-2 text-sm font-semibold">This agent ({entries.length})</h2>
        <ScrollArea className="h-[min(50vh,420px)] pr-3">
          <ul className="space-y-3">
            {entries.map((e) => (
              <li key={e.id} className="rounded-md border border-admin-border bg-muted/20 p-3 text-sm">
                <div className="flex flex-wrap items-start justify-between gap-2">
                  <div className="min-w-0 flex-1">
                    <div className="font-medium">{e.title}</div>
                    <div className="mt-1 flex flex-wrap gap-1">
                      {e.tags?.length
                        ? e.tags.map((t) => (
                            <span
                              key={t}
                              className="rounded border border-admin-border px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground"
                            >
                              {t}
                            </span>
                          ))
                        : null}
                      <span className="font-mono text-[10px] text-muted-foreground">{e.source}</span>
                    </div>
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-xs"
                    className="shrink-0 text-muted-foreground hover:text-destructive"
                    title="Delete entry"
                    onClick={async () => {
                      if (!window.confirm(`Delete “${e.title}”?`)) return;
                      setError(null);
                      try {
                        const r = await fetch(
                          companyOsUrl(apiBase, `/api/company/companies/${companyId}/memory/${e.id}/delete`),
                          { method: "POST" },
                        );
                        const j = (await r.json().catch(() => ({}))) as { error?: string };
                        if (!r.ok) throw new Error(j.error ?? `${r.status}`);
                        await refetch(searchApplied);
                      } catch (err) {
                        setError(err instanceof Error ? err.message : String(err));
                      }
                    }}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </Button>
                </div>
                {e.summary_l1 ? <p className="mt-2 text-xs text-muted-foreground">{e.summary_l1}</p> : null}
                <pre className="mt-2 max-h-36 overflow-auto whitespace-pre-wrap font-mono text-[11px] text-muted-foreground">
                  {e.body}
                </pre>
                <div className="mt-2 font-mono text-[10px] text-muted-foreground">Updated {e.updated_at}</div>
              </li>
            ))}
            {!entries.length && !loading ? (
              <li className="text-sm text-muted-foreground">
                No agent-scoped rows yet. Add preferences, standing instructions, or facts only this pack should
                see in tasks.
              </li>
            ) : null}
          </ul>
        </ScrollArea>
      </div>
    </div>
  );
}
