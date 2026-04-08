"use client";

import { useCallback, useEffect, useState } from "react";
import Link from "next/link";
import { LayoutList, Sparkles } from "lucide-react";

const STORAGE_KEY = "hsm-ws-quickstart-collapsed";

/**
 * Short, skimmable orientation for operators — dismissible so power users reclaim vertical space.
 */
export function WorkspaceQuickStart() {
  const [open, setOpen] = useState(true);

  useEffect(() => {
    try {
      if (typeof window !== "undefined" && window.localStorage.getItem(STORAGE_KEY) === "1") {
        setOpen(false);
      }
    } catch {
      /* ignore */
    }
  }, []);

  const collapse = useCallback(() => {
    setOpen(false);
    try {
      window.localStorage.setItem(STORAGE_KEY, "1");
    } catch {
      /* ignore */
    }
  }, []);

  if (!open) {
    return (
      <button
        type="button"
        onClick={() => {
          setOpen(true);
          try {
            window.localStorage.removeItem(STORAGE_KEY);
          } catch {
            /* ignore */
          }
        }}
        className="mb-3 flex w-full items-center gap-2 rounded-lg border border-[#333333] bg-[#111111] px-3 py-2 text-left font-mono text-[11px] text-[#888888] transition-colors hover:border-[#444444] hover:text-[#cccccc]"
      >
        <Sparkles className="size-3.5 shrink-0 text-[#79b8ff]/80" aria-hidden />
        Show workspace tips
      </button>
    );
  }

  return (
    <section
      className="mb-5 rounded-xl border border-[#30363D] bg-gradient-to-br from-[#0d1117] to-[#0a0a0a] p-4 shadow-sm"
      aria-labelledby="ws-quickstart-title"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <Sparkles className="size-4 shrink-0 text-[#79b8ff]" aria-hidden />
          <h2 id="ws-quickstart-title" className="text-sm font-medium tracking-tight text-[#e8e8e8]">
            Manage work in three moves
          </h2>
        </div>
        <div className="flex items-center gap-1">
          <button
            type="button"
            onClick={collapse}
            className="rounded px-2 py-1 font-mono text-[10px] uppercase tracking-wide text-[#666666] hover:bg-white/5 hover:text-[#aaaaaa]"
          >
            Dismiss
          </button>
        </div>
      </div>
      <ol className="mt-3 grid gap-3 text-sm leading-relaxed text-[#b0b0b0] sm:grid-cols-3">
        <li className="flex gap-2 rounded-lg border border-white/5 bg-black/20 p-3">
          <span className="font-mono text-[11px] font-semibold text-[#79b8ff]">1</span>
          <div>
            <p className="font-medium text-[#e8e8e8]">Pick a company</p>
            <p className="mt-0.5 text-xs text-[#8B949E]">Use the header selector — everything below is scoped to that workspace.</p>
          </div>
        </li>
        <li className="flex gap-2 rounded-lg border border-white/5 bg-black/20 p-3">
          <span className="font-mono text-[11px] font-semibold text-[#79b8ff]">2</span>
          <div>
            <p className="font-medium text-[#e8e8e8]">Triage work</p>
            <p className="mt-0.5 text-xs text-[#8B949E]">
              Open{" "}
              <Link href="/workspace/issues" className="font-medium text-[#79b8ff] underline-offset-2 hover:underline">
                Issues
              </Link>{" "}
              for tasks, or{" "}
              <Link href="/workspace/approvals" className="font-medium text-[#79b8ff] underline-offset-2 hover:underline">
                Approvals
              </Link>{" "}
              when humans must decide.
            </p>
          </div>
        </li>
        <li className="flex gap-2 rounded-lg border border-white/5 bg-black/20 p-3">
          <span className="font-mono text-[11px] font-semibold text-[#79b8ff]">3</span>
          <div>
            <p className="font-medium text-[#e8e8e8]">Tune agents &amp; context</p>
            <p className="mt-0.5 text-xs text-[#8B949E]">
              <Link href="/workspace/agents" className="font-medium text-[#79b8ff] underline-offset-2 hover:underline">
                Agents
              </Link>{" "}
              for roster + files;{" "}
              <Link href="/workspace/playbooks" className="font-medium text-[#79b8ff] underline-offset-2 hover:underline">
                Playbooks
              </Link>{" "}
              for SOPs. Use{" "}
              <Link href="/" className="font-medium text-[#79b8ff] underline-offset-2 hover:underline">
                Legacy console
              </Link>{" "}
              for shared company Markdown.
            </p>
          </div>
        </li>
      </ol>
      <p className="mt-3 flex items-center gap-1 font-mono text-[10px] text-[#666666]">
        <LayoutList className="size-3" aria-hidden />
        Press <kbd className="rounded border border-[#444444] bg-[#1a1a1a] px-1 py-px">⌘K</kbd> to jump anywhere.
      </p>
    </section>
  );
}
