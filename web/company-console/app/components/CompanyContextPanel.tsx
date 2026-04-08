"use client";

import { ChevronRight } from "lucide-react";
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
    <details className="group mb-6 rounded-xl border border-[#30363D] bg-[#0d1117]" open>
      <summary className="flex cursor-pointer list-none items-start gap-2 px-4 py-3.5 marker:content-none [&::-webkit-details-marker]:hidden">
        <ChevronRight
          className="mt-0.5 h-4 w-4 shrink-0 text-[#8B949E] transition-transform duration-200 group-open:rotate-90"
          aria-hidden
        />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-sm font-medium text-white">Shared context</span>
            <span className="rounded border border-[#58a6ff]/35 bg-[#388bfd]/10 px-2 py-px font-mono text-[10px] font-semibold uppercase tracking-wide text-[#79b8ff]">
              All agents
            </span>
          </div>
          <p className="mt-1 text-xs leading-relaxed text-[#8B949E]">
            Prepended to <code className="text-[11px] text-[#79b8ff]">GET …/llm-context</code> for every task — not a
            substitute for per-agent briefings below.
          </p>
        </div>
      </summary>
      <div className="space-y-3 border-t border-[#30363D] px-4 py-4">
        <p className="text-sm leading-relaxed text-[#8B949E]">
          Paste <strong className="font-medium text-[#c9d1d9]">Markdown</strong> the whole workspace should always know:
          policies, fee tables, contacts, product truths. Keep it stable; put fast-changing detail in tasks or agent
          briefings.
        </p>
        <textarea
          className="min-h-[220px] w-full rounded-lg border border-[#30363D] bg-[#010409] px-3 py-2 font-mono text-sm text-[#E8E8E8] outline-none focus:border-[#58a6ff]"
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
