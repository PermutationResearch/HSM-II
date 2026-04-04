"use client";

import { useCallback, useEffect, useState } from "react";

type Props = {
  api: string;
  companyId: string;
  /** From GET /companies list (same row as workspace chips). */
  contextMarkdown: string | null | undefined;
  setCoErr: (msg: string | null) => void;
  onSaved: () => Promise<void>;
};

export function CompanyContextPanel({ api, companyId, contextMarkdown, setCoErr, onSaved }: Props) {
  const [draft, setDraft] = useState(contextMarkdown ?? "");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setDraft(contextMarkdown ?? "");
  }, [companyId, contextMarkdown]);

  const save = useCallback(async () => {
    setCoErr(null);
    setSaving(true);
    try {
      const r = await fetch(`${api}/api/company/companies/${companyId}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ context_markdown: draft }),
      });
      const j = (await r.json()) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? r.statusText);
      await onSaved();
    } catch (e) {
      setCoErr(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  }, [api, companyId, draft, onSaved, setCoErr]);

  return (
    <details className="mb-6 rounded-lg border border-line bg-panel" open>
      <summary className="cursor-pointer list-none px-4 py-3 text-sm font-medium text-gray-200 marker:content-none [&::-webkit-details-marker]:hidden">
        <span className="text-gray-400">▸</span> Company context{" "}
        <span className="font-normal text-gray-500">
          (feeds every task&apos;s <code className="text-xs text-accent">GET …/llm-context</code>)
        </span>
      </summary>
      <div className="space-y-3 border-t border-line px-4 py-4">
        <p className="text-sm leading-relaxed text-gray-500">
          Paste <strong className="font-medium text-gray-400">Markdown</strong> the workspace should always see:
          declaration excerpts, règlement summaries, fee tables, contacts, and internal rules. This is stored on the
          company row and <strong className="font-medium text-gray-400">prepended</strong> to the workforce agent
          profile when integrators call{" "}
          <code className="rounded bg-white/5 px-1 font-mono text-[11px]">/api/company/tasks/&lt;id&gt;/llm-context</code>
          . It does not replace per-agent briefings.
        </p>
        <textarea
          className="min-h-[200px] w-full rounded-lg border border-line bg-ink px-3 py-2 font-mono text-sm text-gray-200"
          placeholder="# Syndicate context&#10;…"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          spellCheck={false}
        />
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            className="rounded-full bg-white px-4 py-2 text-sm font-medium text-black hover:bg-gray-200 disabled:opacity-50"
            disabled={saving}
            onClick={() => void save()}
          >
            {saving ? "Saving…" : "Save company context"}
          </button>
          <span className="text-xs text-gray-600">
            {draft.length.toLocaleString()} chars · included in export bundle
          </span>
        </div>
      </div>
    </details>
  );
}
