"use client";

import { Download, Loader2, Plus, RefreshCw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { companyOsUrl } from "@/app/lib/company-api-url";

export type CompanyMemoryEntry = {
  id: string;
  company_id: string;
  scope: string;
  company_agent_id?: string | null;
  title: string;
  body: string;
  tags: string[];
  source: string;
  summary_l0?: string | null;
  summary_l1?: string | null;
  kind?: string;
  created_at: string;
  updated_at: string;
};

type Props = {
  apiBase: string;
  companyId: string | null;
  postgresOk: boolean;
  onError: (msg: string | null) => void;
};

export function CompanySharedMemoryPanel({ apiBase, companyId, postgresOk, onError }: Props) {
  const [entries, setEntries] = useState<CompanyMemoryEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [searchDraft, setSearchDraft] = useState("");
  const [searchApplied, setSearchApplied] = useState("");
  const [newTitle, setNewTitle] = useState("");
  const [newBody, setNewBody] = useState("");
  const [newTags, setNewTags] = useState("");
  const [newKind, setNewKind] = useState<"general" | "broadcast">("general");
  const [saving, setSaving] = useState(false);

  const refetch = useCallback(
    async (needle: string) => {
      if (!companyId || !postgresOk) {
        setEntries([]);
        return;
      }
      setLoading(true);
      onError(null);
      try {
        const sp = new URLSearchParams({ scope: "shared" });
        const n = needle.trim();
        if (n) sp.set("q", n);
        const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/memory?${sp}`));
        const j = (await r.json().catch(() => ({}))) as { entries?: CompanyMemoryEntry[]; error?: string };
        if (!r.ok) throw new Error(j.error ?? `memory ${r.status}`);
        setEntries(j.entries ?? []);
      } catch (e) {
        onError(e instanceof Error ? e.message : String(e));
        setEntries([]);
      } finally {
        setLoading(false);
      }
    },
    [apiBase, companyId, postgresOk, onError]
  );

  useEffect(() => {
    void refetch(searchApplied);
  }, [refetch, searchApplied]);

  if (!postgresOk) {
    return (
      <div className="rounded-lg border border-amber-900/40 bg-amber-950/20 px-4 py-3 text-sm text-amber-100/90">
        Company shared memory needs Postgres. Set <code className="font-mono text-[11px]">HSM_COMPANY_OS_DATABASE_URL</code>{" "}
        and restart <code className="font-mono text-[11px]">hsm_console</code>.
      </div>
    );
  }

  if (!companyId) {
    return (
      <div className="rounded-lg border border-line bg-panel px-4 py-3 text-sm text-gray-400">
        Select a workspace from the sidebar to view or edit shared memories.
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-end gap-3">
        <div className="min-w-[200px] flex-1">
          <label className="mb-1 block font-mono text-[10px] uppercase tracking-wide text-gray-500">Search</label>
          <p className="mb-1 text-[10px] text-gray-600">
            Uses Postgres full-text + substring match on title, body, and summaries.
          </p>
          <input
            className="w-full rounded border border-line bg-panel px-3 py-2 text-sm text-white placeholder:text-gray-600"
            placeholder="Title or body…"
            value={searchDraft}
            onChange={(e) => setSearchDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                setSearchApplied(searchDraft.trim());
              }
            }}
          />
        </div>
        <button
          type="button"
          disabled={loading}
          className="inline-flex items-center gap-2 rounded border border-line bg-ink px-3 py-2 text-sm text-gray-300 hover:bg-white/5 disabled:opacity-50"
          onClick={() => setSearchApplied(searchDraft.trim())}
        >
          Search
        </button>
        <button
          type="button"
          disabled={loading}
          className="inline-flex items-center gap-2 rounded border border-line bg-ink px-3 py-2 text-sm text-gray-300 hover:bg-white/5 disabled:opacity-50"
          onClick={() => void refetch(searchApplied)}
        >
          {loading ? <Loader2 className="h-4 w-4 animate-spin" /> : <RefreshCw className="h-4 w-4" />}
          Refresh
        </button>
        <a
          href={companyOsUrl(apiBase, `/api/company/companies/${companyId}/memory/export.md`)}
          target="_blank"
          rel="noreferrer"
          className="inline-flex items-center gap-2 rounded border border-[#388bfd]/40 bg-[#388bfd]/10 px-3 py-2 text-sm text-[#79b8ff] hover:bg-[#388bfd]/20"
        >
          <Download className="h-4 w-4" />
          export.md
        </a>
      </div>

      <div className="rounded-lg border border-line bg-panel p-4">
        <h2 className="mb-1 text-sm font-semibold text-white">Add shared memory</h2>
        <p className="mb-3 text-xs text-gray-500">
          These entries are injected into task LLM context for every agent on this workspace (after company markdown).
        </p>
        <input
          className="mb-2 w-full rounded border border-line bg-ink px-3 py-2 text-sm"
          placeholder="Title"
          value={newTitle}
          onChange={(e) => setNewTitle(e.target.value)}
        />
        <textarea
          className="mb-2 w-full rounded border border-line bg-ink px-3 py-2 text-sm"
          placeholder="Body (markdown ok)"
          rows={4}
          value={newBody}
          onChange={(e) => setNewBody(e.target.value)}
        />
        <input
          className="mb-2 w-full rounded border border-line bg-ink px-3 py-2 text-sm"
          placeholder="Tags (comma-separated, optional)"
          value={newTags}
          onChange={(e) => setNewTags(e.target.value)}
        />
        <label className="mb-3 flex flex-wrap items-center gap-2 text-xs text-gray-400">
          <span>Kind</span>
          <select
            className="rounded border border-line bg-ink px-2 py-1 text-sm text-white"
            value={newKind}
            onChange={(e) => setNewKind(e.target.value as "general" | "broadcast")}
          >
            <option value="general">general</option>
            <option value="broadcast">broadcast (merged first in LLM context)</option>
          </select>
        </label>
        <button
          type="button"
          disabled={saving || !newTitle.trim()}
          className="inline-flex items-center gap-2 rounded-md border border-[#30363d] bg-[#21262d] px-4 py-2 text-sm font-medium text-white hover:bg-[#30363d] disabled:opacity-50"
          onClick={async () => {
            onError(null);
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
                  scope: "shared",
                  tags,
                  source: "human",
                  kind: newKind,
                }),
              });
              const j = await r.json().catch(() => ({}));
              if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
              setNewTitle("");
              setNewBody("");
              setNewTags("");
              await refetch(searchApplied);
            } catch (e) {
              onError(e instanceof Error ? e.message : String(e));
            } finally {
              setSaving(false);
            }
          }}
        >
          {saving ? <Loader2 className="h-4 w-4 animate-spin" /> : <Plus className="h-4 w-4" />}
          Save entry
        </button>
      </div>

      <div>
        <h2 className="mb-2 text-sm font-semibold text-white">Shared pool ({entries.length})</h2>
        <ul className="max-h-[50vh] space-y-3 overflow-auto pr-1">
          {entries.map((e) => (
            <li key={e.id} className="rounded border border-line bg-ink/40 p-3 text-sm">
              <div className="flex flex-wrap items-start justify-between gap-2">
                <div className="min-w-0 flex-1">
                  <div className="font-medium text-gray-200">{e.title}</div>
                  <div className="mt-1 flex flex-wrap gap-1">
                    {e.kind === "broadcast" ? (
                      <span className="rounded border border-amber-700/50 bg-amber-950/40 px-1.5 py-0.5 font-mono text-[10px] text-amber-200">
                        broadcast
                      </span>
                    ) : null}
                    {e.tags?.length
                      ? e.tags.map((t) => (
                          <span
                            key={t}
                            className="rounded border border-line px-1.5 py-0.5 font-mono text-[10px] text-gray-400"
                          >
                            {t}
                          </span>
                        ))
                      : null}
                    <span className="font-mono text-[10px] text-gray-600">{e.source}</span>
                  </div>
                </div>
                <button
                  type="button"
                  title="Delete entry"
                  className="shrink-0 rounded border border-red-900/50 p-1.5 text-red-300 hover:bg-red-950/30"
                  onClick={async () => {
                    if (!window.confirm(`Delete “${e.title}”?`)) return;
                    onError(null);
                    try {
                      const r = await fetch(
                        companyOsUrl(apiBase, `/api/company/companies/${companyId}/memory/${e.id}/delete`),
                        { method: "POST" },
                      );
                      const j = await r.json().catch(() => ({}));
                      if (!r.ok) throw new Error((j as { error?: string }).error ?? r.statusText);
                      await refetch(searchApplied);
                    } catch (err) {
                      onError(err instanceof Error ? err.message : String(err));
                    }
                  }}
                >
                  <Trash2 className="h-4 w-4" />
                </button>
              </div>
              {e.summary_l1 ? (
                <p className="mt-2 text-xs text-gray-500">{e.summary_l1}</p>
              ) : null}
              <pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap font-mono text-[11px] text-gray-400">
                {e.body}
              </pre>
              <div className="mt-2 font-mono text-[10px] text-gray-600">Updated {e.updated_at}</div>
            </li>
          ))}
          {!entries.length && !loading ? (
            <li className="text-sm text-gray-500">No entries yet. Add policy notes, canonical paths, or onboarding facts.</li>
          ) : null}
        </ul>
      </div>
    </div>
  );
}
