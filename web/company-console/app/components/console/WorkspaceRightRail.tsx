"use client";

import Link from "next/link";
import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState, memo } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  Bot,
  ChevronRight,
  FilePlus,
  FileText,
  FolderOpen,
  FolderPlus,
  MessageSquare,
  RefreshCw,
  Send,
  Trash2,
  X,
} from "lucide-react";

import { Badge } from "@/app/components/ui/badge";
import { Button } from "@/app/components/ui/button";
import { Input } from "@/app/components/ui/input";
import { Textarea } from "@/app/components/ui/textarea";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import type { HsmTaskRow } from "@/app/lib/hsm-api-types";
import {
  callResumeRun,
  createCompanyTask,
  getAgentRun,
  listCompanyTasks,
  markTaskRequiresHuman,
  patchAgentRun,
  postRunFeedback,
  postTaskStigmergicNote,
  promoteRunFeedbackToTask,
} from "@/app/lib/company-runtime-client";
import { getAgentChatReplyStreamUrl } from "@/app/lib/console-api-base";
import { useCompanyAgents, useCompanyTasks } from "@/app/lib/hsm-queries";
import {
  asArray,
  asObject,
  canTransitionRunLoopState,
  parseExecutionMode,
  parseRunLoopState,
  parseRunStatus,
  type RunStatus as ContractRunStatus,
} from "@/app/lib/runtime-contract";
import { cn } from "@/app/lib/utils";
import { extractAnthropicStreamTextEffect } from "@/app/lib/claude-stream-shape";
import { buildIssueSpecFromPlan, buildIssueTitleFromPlan, isDoneTask, isPlanTask } from "@/app/lib/workspace-issue";

/** Markdown body for file preview and agent transcript (stream + notes) — same styling end-to-end. */
const WORKSPACE_MARKDOWN_PROSE_CN = cn(
  "min-w-0 text-[13px] leading-relaxed text-[#c8c8c8]",
  "[&_a]:text-[#7ab8ff] [&_a]:underline-offset-2 hover:[&_a]:underline",
  "[&_code]:rounded [&_code]:bg-[#1a1a1a] [&_code]:px-1 [&_code]:text-[12px] [&_code]:text-[#e8c96b]",
  "[&_pre]:my-2 [&_pre]:overflow-x-auto [&_pre]:rounded-md [&_pre]:bg-[#0a0a0a] [&_pre]:p-2 [&_pre]:text-[11px]",
  "[&_ul]:my-2 [&_ul]:list-disc [&_ul]:pl-5",
  "[&_ol]:my-2 [&_ol]:list-decimal [&_ol]:pl-5",
  "[&_h1]:mb-2 [&_h1]:text-[15px] [&_h1]:font-semibold [&_h2]:mb-1 [&_h2]:mt-3 [&_h2]:text-[14px] [&_h2]:font-semibold",
  "[&_blockquote]:border-l-2 [&_blockquote]:border-[#444444] [&_blockquote]:pl-3 [&_blockquote]:text-[#999999]",
  "[&_p]:my-1 [&_p:first-child]:mt-0 [&_p:last-child]:mb-0",
);

const WorkspaceMarkdownBody = memo(function WorkspaceMarkdownBody({
  text,
  className,
}: {
  text: string;
  className?: string;
}) {
  return (
    <div className={cn(WORKSPACE_MARKDOWN_PROSE_CN, className)}>
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{text}</ReactMarkdown>
    </div>
  );
});

/** Defers markdown reconciliation so high-frequency token updates do not block the main thread each chunk. */
function WorkspaceMarkdownStreamBody({ text, className }: { text: string; className?: string }) {
  const deferred = useDeferredValue(text);
  return <WorkspaceMarkdownBody text={deferred} className={className} />;
}

type RunStatus = ContractRunStatus;
type LiveRun = {
  runId: string;
  skill: string;
  status: RunStatus;
  summary: string | null;
  taskId?: string | null;
  executionMode?: "worker" | "llm_simulated" | "pending" | "unknown";
};
type RuntimeToolEvent = {
  event_type?: string;
  task_key?: string | null;
  tool_name?: string | null;
  call_id?: string | null;
  success?: boolean;
  message?: string;
  ts_ms?: number;
};
type RunTimelinePhase =
  | "run_start"
  | "tool_start"
  | "tool_complete"
  | "tool_error"
  | "checkpoint"
  | "resume"
  | "run_status";
type RunTimelineEntry = {
  seq: number;
  runId: string;
  tsMs: number;
  phase: RunTimelinePhase;
  message: string;
  toolName?: string | null;
  callId?: string | null;
};
type ChatTranscriptItem =
  | { kind: "note"; key: string; note: StigNote; typing: boolean }
  | { kind: "tool"; key: string; event: RuntimeToolEvent }
  | { kind: "status"; key: string; text: string }
  | { kind: "assistant_partial"; key: string; text: string };

const CHANNEL_STORAGE_KEY = "pc-ws-agent-channels-v1";

type StigNote = { at: string; actor: string; text: string };

type ChannelPersist = { taskId: string; notes: StigNote[] };

function loadChannels(companyId: string): Record<string, ChannelPersist> {
  if (typeof window === "undefined") return {};
  try {
    const raw = sessionStorage.getItem(CHANNEL_STORAGE_KEY);
    if (!raw) return {};
    const all = JSON.parse(raw) as Record<string, Record<string, ChannelPersist>>;
    return all[companyId] ?? {};
  } catch {
    return {};
  }
}

function saveChannel(companyId: string, persona: string, data: ChannelPersist) {
  if (typeof window === "undefined") return;
  try {
    const raw = sessionStorage.getItem(CHANNEL_STORAGE_KEY);
    const all = (raw ? JSON.parse(raw) : {}) as Record<string, Record<string, ChannelPersist>>;
    if (!all[companyId]) all[companyId] = {};
    all[companyId][persona] = data;
    sessionStorage.setItem(CHANNEL_STORAGE_KEY, JSON.stringify(all));
  } catch {
    /* ignore */
  }
}

function parseNotesFromResponse(v: unknown): StigNote[] {
  if (!Array.isArray(v)) return [];
  const out: StigNote[] = [];
  for (const item of v) {
    if (!item || typeof item !== "object") continue;
    const o = item as Record<string, unknown>;
    const text = typeof o.text === "string" ? o.text : "";
    if (!text) continue;
    out.push({
      at: typeof o.at === "string" ? o.at : "",
      actor: typeof o.actor === "string" ? o.actor : "operator",
      text,
    });
  }
  return out;
}

function taskActivityTs(t: HsmTaskRow): number {
  const u = t.run?.updated_at;
  if (u) {
    const n = Date.parse(u);
    if (Number.isFinite(n)) return n;
  }
  return 0;
}

function findBestTaskForPersona(tasks: HsmTaskRow[], persona: string): string | null {
  const p = persona.trim();
  if (!p) return null;
  const matches = tasks.filter((t) => {
    const op = (t.owner_persona ?? "").trim();
    const cb = (t.checked_out_by ?? "").trim();
    return op === p || cb === p;
  });
  matches.sort((a, b) => {
    const d = taskActivityTs(b) - taskActivityTs(a);
    if (d !== 0) return d;
    return b.id.localeCompare(a.id);
  });
  return matches[0]?.id ?? null;
}

type AgentRow = {
  persona: string;
  registryId: string | null;
  liveCount: number;
  title: string | null;
  role: string | null;
};

function rowSubtitle(row: AgentRow): string {
  const t = row.title?.trim();
  const r = row.role?.trim();
  if (t && r && t.toLowerCase() !== r.toLowerCase()) return `${t} · ${r}`;
  if (t) return t;
  if (r) return r;
  return row.registryId ? "Workforce roster" : "From task assignees";
}

function isoToMs(value: string): number {
  const ms = Date.parse(value);
  return Number.isFinite(ms) ? ms : 0;
}

function formatRuntimeEventLabel(event: RuntimeToolEvent): string {
  const isErr = event.success === false || /error|fail|blocked|denied/i.test(event.message ?? "");
  if (event.event_type === "tool_start") return "tool start";
  return isErr ? "tool error" : "tool result";
}

type WorkspaceListEntry = {
  name: string;
  path: string;
  kind: "dir" | "file";
  size_bytes?: number | null;
};

/** Open file in workspace rail: editable text, image preview, or opaque binary. */
type WorkspaceBrowserSelection =
  | { kind: "text"; path: string; name: string; content: string }
  | { kind: "image"; path: string; name: string; dataUrl: string; mimeType: string; byteLen: number }
  | { kind: "binary"; path: string; name: string; mimeType: string; byteLen: number };

function joinWorkspaceRel(cwd: string, name: string): string {
  const n = name.trim().replace(/\\/g, "/").replace(/^\/+/, "");
  if (!n) return cwd;
  return cwd ? `${cwd}/${n}` : n;
}

function isUnderRecycle(relPath: string): boolean {
  const p = relPath.replace(/\\/g, "/");
  return p === ".recycle" || p.startsWith(".recycle/");
}

function WorkspaceRailFileBrowser({
  apiBase,
  companyId,
}: {
  apiBase: string;
  companyId: string;
}) {
  const [cwd, setCwd] = useState("");
  const [entries, setEntries] = useState<WorkspaceListEntry[]>([]);
  const [listRev, setListRev] = useState(0);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [selected, setSelected] = useState<WorkspaceBrowserSelection | null>(null);
  const [editContent, setEditContent] = useState("");
  const [showMdPreview, setShowMdPreview] = useState(false);
  const [ioBusy, setIoBusy] = useState<string | null>(null);
  const [createMode, setCreateMode] = useState<null | "file" | "folder">(null);
  const [createName, setCreateName] = useState("");

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setErr(null);
    void fetch(
      `${apiBase}/api/company/companies/${companyId}/workspace/list?path=${encodeURIComponent(cwd)}`,
    )
      .then(async (r) => {
        const raw = await r.json().catch(() => ({}));
        const j = asObject(raw);
        const error = typeof j?.error === "string" ? j.error : undefined;
        const entries = asArray(j?.entries) as WorkspaceListEntry[];
        if (!r.ok) throw new Error(error ?? r.statusText);
        if (!cancelled) setEntries(entries);
      })
      .catch((e) => {
        if (!cancelled) setErr(e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [apiBase, companyId, cwd, listRev]);

  useEffect(() => {
    if (selected?.kind === "text") {
      setEditContent(selected.content);
      setShowMdPreview(false);
    } else {
      setEditContent("");
    }
  }, [selected]);

  const sortedEntries = useMemo(() => {
    const out = [...entries];
    out.sort((a, b) => {
      const aRec = a.kind === "dir" && a.name === ".recycle" ? 1 : 0;
      const bRec = b.kind === "dir" && b.name === ".recycle" ? 1 : 0;
      if (aRec !== bRec) return bRec - aRec;
      const ad = a.kind === "dir" ? 1 : 0;
      const bd = b.kind === "dir" ? 1 : 0;
      if (ad !== bd) return bd - ad;
      return a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
    });
    return out;
  }, [entries]);

  const openFile = useCallback(
    async (path: string, name: string) => {
      setErr(null);
      try {
        const r = await fetch(
          `${apiBase}/api/company/companies/${companyId}/workspace/file?path=${encodeURIComponent(path)}`,
        );
        const raw = await r.json().catch(() => ({}));
        const j = asObject(raw);
        const error = typeof j?.error === "string" ? j.error : undefined;
        if (!r.ok) throw new Error(error ?? r.statusText);
        const enc = typeof j?.encoding === "string" ? j.encoding : "utf-8";
        if (enc === "base64") {
          const b64 = typeof j?.content_base64 === "string" ? j.content_base64 : "";
          const mime = typeof j?.mime_type === "string" ? j.mime_type : "application/octet-stream";
          const byteLen = typeof j?.byte_len === "number" ? j.byte_len : 0;
          if (mime.startsWith("image/")) {
            setSelected({
              kind: "image",
              path,
              name,
              mimeType: mime,
              byteLen,
              dataUrl: `data:${mime};base64,${b64}`,
            });
          } else {
            setSelected({ kind: "binary", path, name, mimeType: mime, byteLen });
          }
        } else {
          const content = typeof j?.content === "string" ? j.content : "";
          setSelected({ kind: "text", path, name, content });
        }
      } catch (e) {
        setErr(e instanceof Error ? e.message : String(e));
      }
    },
    [apiBase, companyId],
  );

  const saveFile = useCallback(async () => {
    if (!selected || selected.kind !== "text") return;
    setIoBusy("save");
    setErr(null);
    try {
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/workspace/file`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ path: selected.path, content: editContent }),
      });
      const raw = await r.json().catch(() => ({}));
      const j = asObject(raw);
      const error = typeof j?.error === "string" ? j.error : undefined;
      if (!r.ok) throw new Error(error ?? r.statusText);
      setSelected({ kind: "text", path: selected.path, name: selected.name, content: editContent });
      setListRev((n) => n + 1);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setIoBusy(null);
    }
  }, [apiBase, companyId, editContent, selected]);

  const trashFile = useCallback(async () => {
    if (!selected) return;
    if (!window.confirm(`Move “${selected.name}” to the recycle folder (.recycle)?`)) return;
    setIoBusy("trash");
    setErr(null);
    try {
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/workspace/file/trash`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ path: selected.path }),
      });
      const raw = await r.json().catch(() => ({}));
      const j = asObject(raw);
      const error = typeof j?.error === "string" ? j.error : undefined;
      if (!r.ok) throw new Error(error ?? r.statusText);
      setSelected(null);
      setListRev((n) => n + 1);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setIoBusy(null);
    }
  }, [apiBase, companyId, selected]);

  const purgeFile = useCallback(async () => {
    if (!selected || !isUnderRecycle(selected.path)) return;
    if (!window.confirm(`Permanently delete “${selected.name}”? This cannot be undone.`)) return;
    setIoBusy("purge");
    setErr(null);
    try {
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/workspace/file/delete`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ path: selected.path }),
      });
      const raw = await r.json().catch(() => ({}));
      const j = asObject(raw);
      const error = typeof j?.error === "string" ? j.error : undefined;
      if (!r.ok) throw new Error(error ?? r.statusText);
      setSelected(null);
      setListRev((n) => n + 1);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setIoBusy(null);
    }
  }, [apiBase, companyId, selected]);

  const openRecycleBin = useCallback(async () => {
    setErr(null);
    try {
      const mk = await fetch(`${apiBase}/api/company/companies/${companyId}/workspace/mkdir`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ path: ".recycle" }),
      });
      const raw = await mk.json().catch(() => ({}));
      const j = asObject(raw);
      const error = typeof j?.error === "string" ? j.error : undefined;
      if (!mk.ok) throw new Error(error ?? mk.statusText);
      setCwd(".recycle");
      setSelected(null);
      setCreateMode(null);
      setCreateName("");
      setListRev((n) => n + 1);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, [apiBase, companyId]);

  const submitCreate = useCallback(async () => {
    const name = createName.trim();
    if (!name) {
      setErr("Name required.");
      return;
    }
    const rel = joinWorkspaceRel(cwd, name);
    setIoBusy(createMode === "file" ? "create_file" : "mkdir");
    setErr(null);
    try {
      if (createMode === "folder") {
        const r = await fetch(`${apiBase}/api/company/companies/${companyId}/workspace/mkdir`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ path: rel }),
        });
        const raw = await r.json().catch(() => ({}));
        const j = asObject(raw);
        const error = typeof j?.error === "string" ? j.error : undefined;
        if (!r.ok) throw new Error(error ?? r.statusText);
        setCreateMode(null);
        setCreateName("");
        setListRev((n) => n + 1);
      } else {
        const r = await fetch(`${apiBase}/api/company/companies/${companyId}/workspace/file`, {
          method: "PUT",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ path: rel, content: "" }),
        });
        const raw = await r.json().catch(() => ({}));
        const j = asObject(raw);
        const error = typeof j?.error === "string" ? j.error : undefined;
        if (!r.ok) throw new Error(error ?? r.statusText);
        setCreateMode(null);
        setCreateName("");
        const base = name.split("/").pop() ?? name;
        setListRev((n) => n + 1);
        await openFile(rel, base);
      }
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setIoBusy(null);
    }
  }, [apiBase, companyId, createMode, createName, cwd, openFile]);

  const crumbs = cwd ? cwd.split("/").filter(Boolean) : [];
  const inRecycle = isUnderRecycle(cwd);
  const dirty = selected?.kind === "text" && editContent !== selected.content;

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div className="shrink-0 border-b border-[#222222] px-3 py-2">
        <div className="flex flex-wrap items-center gap-1 font-mono text-[10px] text-[#888888]">
          <button
            type="button"
            className="rounded px-1 text-[#c8c8c8] hover:bg-white/[0.06]"
            onClick={() => {
              setCwd("");
              setSelected(null);
              setCreateMode(null);
              setCreateName("");
            }}
          >
            workspace
          </button>
          {crumbs.map((seg, i) => {
            const prefix = crumbs.slice(0, i + 1).join("/");
            return (
              <span key={prefix} className="flex items-center gap-1">
                <ChevronRight className="size-3 text-[#555555]" aria-hidden />
                <button
                  type="button"
                  className="rounded px-1 text-[#c8c8c8] hover:bg-white/[0.06]"
                  onClick={() => {
                    setCwd(prefix);
                    setSelected(null);
                    setCreateMode(null);
                    setCreateName("");
                  }}
                >
                  {seg}
                </button>
              </span>
            );
          })}
        </div>
        {!selected ? (
          <div className="mt-2.5 flex w-full gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={!!ioBusy}
              title="Create a new file in the current folder"
              className="h-8 min-w-0 flex-1 justify-center gap-1.5 border-[#2a2a2a] bg-[#0a0a0a] px-1.5 font-mono text-[9px] font-normal uppercase leading-tight tracking-wide text-[#b8b8b8] hover:border-[#3d3d3d] hover:bg-white/[0.04] hover:text-[#e8e8e8]"
              onClick={() => {
                setCreateMode("file");
                setCreateName("");
              }}
            >
              <FilePlus className="size-3.5 shrink-0 opacity-80" strokeWidth={1.5} aria-hidden />
              <span className="text-center">New file</span>
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={!!ioBusy}
              title="Create a new folder"
              className="h-8 min-w-0 flex-1 justify-center gap-1.5 border-[#2a2a2a] bg-[#0a0a0a] px-1.5 font-mono text-[9px] font-normal uppercase leading-tight tracking-wide text-[#b8b8b8] hover:border-[#3d3d3d] hover:bg-white/[0.04] hover:text-[#e8e8e8]"
              onClick={() => {
                setCreateMode("folder");
                setCreateName("");
              }}
            >
              <FolderPlus className="size-3.5 shrink-0 opacity-80" strokeWidth={1.5} aria-hidden />
              <span className="text-center">New folder</span>
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={!!ioBusy}
              title="Open .recycle folder"
              className="h-8 min-w-0 flex-1 justify-center gap-1.5 border-[#2a2a2a] bg-[#0a0a0a] px-1.5 font-mono text-[9px] font-normal uppercase leading-tight tracking-wide text-[#b8b8b8] hover:border-[#3d3d3d] hover:bg-white/[0.04] hover:text-[#e8e8e8]"
              onClick={() => void openRecycleBin()}
            >
              <Trash2 className="size-3.5 shrink-0 opacity-70" strokeWidth={1.5} aria-hidden />
              <span className="text-center">Recycle</span>
            </Button>
          </div>
        ) : null}
        {createMode && !selected ? (
          <div className="mt-2 flex flex-wrap items-end gap-2">
            <div className="min-w-0 flex-1">
              <label className="mb-0.5 block font-mono text-[9px] uppercase tracking-wide text-[#666666]">
                {createMode === "file" ? "File name (under current folder)" : "Folder name"}
              </label>
              <Input
                value={createName}
                onChange={(e) => setCreateName(e.target.value)}
                placeholder={createMode === "file" ? "notes.md" : "my-folder"}
                className="h-8 border-[#333333] bg-black font-mono text-xs text-[#e8e8e8]"
                onKeyDown={(e) => {
                  if (e.key === "Enter") void submitCreate();
                }}
              />
            </div>
            <Button
              type="button"
              size="sm"
              disabled={!!ioBusy}
              className="h-8 border-[#333333] bg-black font-mono text-[10px] uppercase tracking-wide text-[#e8e8e8] hover:bg-white/[0.08]"
              onClick={() => void submitCreate()}
            >
              Create
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-8 font-mono text-[10px] uppercase tracking-wide text-[#666666]"
              onClick={() => {
                setCreateMode(null);
                setCreateName("");
              }}
            >
              Cancel
            </Button>
          </div>
        ) : null}
        {selected ? (
          <div className="mt-2.5 space-y-2.5">
            <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
              <p
                className="min-w-0 font-mono text-[10px] leading-snug text-[#999999] sm:max-w-[min(100%,14rem)] sm:truncate"
                title={selected.path}
              >
                {isUnderRecycle(selected.path) ? (
                  <span className="text-[#D4A843]">.recycle · </span>
                ) : null}
                <span className="break-all text-[#c8c8c8] sm:break-normal">{selected.name}</span>
                {dirty ? (
                  <span className="ml-2 font-mono text-[9px] uppercase tracking-wide text-[#D4A843]">
                    unsaved
                  </span>
                ) : null}
              </p>
              <div className="flex shrink-0 flex-wrap items-stretch justify-end gap-2">
                {selected.kind === "text" ? (
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={!!ioBusy || !dirty}
                    className="h-8 border-[#2a2a2a] bg-[#0a0a0a] px-3 font-mono text-[10px] font-normal uppercase tracking-wide text-[#d0d0d0] hover:border-[#3d3d3d] hover:bg-white/[0.04] disabled:border-[#222222] disabled:text-[#555555]"
                    onClick={() => void saveFile()}
                  >
                    Save
                  </Button>
                ) : null}
                {!isUnderRecycle(selected.path) ? (
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={!!ioBusy}
                    className="h-8 border-[#2a2a2a] bg-[#0a0a0a] px-3 font-mono text-[10px] font-normal uppercase tracking-wide text-[#d0d0d0] hover:border-[#3d3d3d] hover:bg-white/[0.04]"
                    onClick={() => void trashFile()}
                  >
                    To recycle
                  </Button>
                ) : (
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={!!ioBusy}
                    className="h-8 border-[#5c4a2a] bg-[#0a0a0a] px-3 font-mono text-[10px] font-normal uppercase tracking-wide text-[#D4A843] hover:border-[#7a6230] hover:bg-[#14100a]"
                    onClick={() => void purgeFile()}
                  >
                    Purge
                  </Button>
                )}
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-8 border-[#2a2a2a] bg-[#0a0a0a] px-3 font-mono text-[10px] font-normal uppercase tracking-wide text-[#888888] hover:border-[#3d3d3d] hover:bg-white/[0.04] hover:text-[#c8c8c8]"
                  onClick={() => setSelected(null)}
                >
                  Close
                </Button>
              </div>
            </div>
            {selected.kind === "text" ? (
              <>
                <Textarea
                  value={editContent}
                  onChange={(e) => setEditContent(e.target.value)}
                  rows={14}
                  spellCheck={false}
                  className="min-h-[200px] resize-y border border-[#2a2a2a] bg-[#050505] font-mono text-[12px] leading-relaxed text-[#e8e8e8] shadow-none placeholder:text-[#555555] focus-visible:border-[#4a4a4a] focus-visible:ring-0 focus-visible:ring-offset-0"
                  aria-label="File contents"
                />
                {/\.md$/i.test(selected.name) ? (
                  <div className="flex flex-col gap-2 border-t border-[#1f1f1f] pt-2 sm:flex-row sm:items-center sm:justify-between">
                    <label className="flex cursor-pointer items-center gap-2 font-mono text-[10px] uppercase tracking-wide text-[#777777]">
                      <input
                        type="checkbox"
                        checked={showMdPreview}
                        onChange={(e) => setShowMdPreview(e.target.checked)}
                        className="size-3.5 rounded border-[#3a3a3a] bg-black accent-[#666666]"
                      />
                      Rendered preview
                    </label>
                    <span className="font-mono text-[9px] uppercase tracking-wide text-[#555555]">Markdown</span>
                  </div>
                ) : null}
                {/\.md$/i.test(selected.name) && showMdPreview ? (
                  <div className="max-h-64 overflow-y-auto rounded-sm border border-[#222222] bg-[#080808] p-3">
                    <WorkspaceMarkdownBody text={editContent} />
                  </div>
                ) : null}
              </>
            ) : selected.kind === "image" ? (
              <div className="space-y-2 rounded-sm border border-[#2a2a2a] bg-[#080808] p-3">
                <p className="font-mono text-[10px] uppercase tracking-wide text-[#666666]">
                  {selected.mimeType} · {(selected.byteLen / 1024).toFixed(1)} KiB · not editable here
                </p>
                <img
                  src={selected.dataUrl}
                  alt={selected.name}
                  className="max-h-[min(50vh,420px)] w-auto max-w-full rounded-sm object-contain"
                />
              </div>
            ) : (
              <div className="rounded-sm border border-[#2a2a2a] bg-[#080808] px-3 py-6 text-center">
                <p className="font-mono text-[11px] leading-relaxed text-[#999999]">
                  Binary file ({selected.mimeType})
                </p>
                <p className="mt-1 font-mono text-[10px] text-[#666666]">
                  {(selected.byteLen / 1024).toFixed(1)} KiB — open on disk or use a desktop tool. You can still move
                  to recycle or purge.
                </p>
              </div>
            )}
          </div>
        ) : null}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {err ? (
          <p className="p-3 font-mono text-[10px] uppercase tracking-wide text-[#D4A843]">[ERROR: {err}]</p>
        ) : null}
        {loading ? (
          <p className="p-3 font-mono text-[10px] uppercase tracking-wide text-[#666666]">[LOADING…]</p>
        ) : null}
        {!selected && !loading ? (
          <ul className="divide-y divide-[#1a1a1a]">
            {inRecycle ? (
              <li className="px-3 py-2 font-mono text-[10px] uppercase tracking-wide text-[#666666]">
                Recovered names include a timestamp prefix. Delete permanently to remove.
              </li>
            ) : null}
            {sortedEntries.length === 0 ? (
              <li className="px-3 py-4 text-center text-xs text-[#666666]">Empty folder</li>
            ) : (
              sortedEntries.map((e) => (
                <li key={e.path}>
                  <button
                    type="button"
                    className="flex w-full items-center gap-2 px-3 py-2.5 text-left transition-colors hover:bg-white/[0.04]"
                    onClick={() => {
                      if (e.kind === "dir") {
                        setCwd(e.path);
                        setSelected(null);
                        setCreateMode(null);
                        setCreateName("");
                      } else {
                        void openFile(e.path, e.name);
                      }
                    }}
                  >
                    {e.kind === "dir" ? (
                      <FolderOpen
                        className={cn(
                          "size-4 shrink-0",
                          e.name === ".recycle" ? "text-[#D4A843]" : "text-[#888888]",
                        )}
                        strokeWidth={1.5}
                      />
                    ) : (
                      <FileText className="size-4 shrink-0 text-[#666666]" strokeWidth={1.5} />
                    )}
                    <span className="min-w-0 flex-1 truncate font-mono text-[12px] text-[#e8e8e8]">
                      {e.name}
                    </span>
                  </button>
                </li>
              ))
            )}
          </ul>
        ) : null}
      </div>
    </div>
  );
}

function rolePillText(row: AgentRow | undefined): string {
  if (!row) return "AGENT";
  const t = row.title?.trim();
  const r = row.role?.trim();
  const raw = t || r || row.persona;
  return raw.toUpperCase();
}

/** True when this persona has active work: run in flight, lease, or in-progress task state. */
function isPersonaWorking(tasks: HsmTaskRow[], persona: string): boolean {
  const p = persona.trim();
  if (!p) return false;
  for (const t of tasks) {
    const op = (t.owner_persona ?? "").trim();
    const cb = (t.checked_out_by ?? "").trim();
    const forPersona = op === p || cb === p;
    if (!forPersona) continue;
    const runSt = (t.run?.status ?? "").toLowerCase();
    if (runSt === "running") return true;
    if (cb === p) return true;
    const st = (t.state ?? "").toLowerCase();
    if (/progress|doing|in_progress|in progress/.test(st)) return true;
  }
  return false;
}

export function WorkspaceRightRail() {
  const { apiBase, companyId, propertiesSelection, setPropertiesSelection, refreshWorkspace } = useWorkspace();
  const qc = useQueryClient();

  const { data: agents = [], isLoading: agentsLoading } = useCompanyAgents(apiBase, companyId);
  const { data: tasks = [], isLoading: tasksLoading } = useCompanyTasks(apiBase, companyId);

  const [channels, setChannels] = useState<Record<string, ChannelPersist>>({});
  const [draft, setDraft] = useState("");
  const [sendErr, setSendErr] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [thinking, setThinking] = useState(false);
  /** Token / partial assistant text from NDJSON stream (OpenRouter deltas or future worker tokens). */
  const [streamAssistantBuf, setStreamAssistantBuf] = useState("");
  const [brailleFrame, setBrailleFrame] = useState(0);
  const [typeoutNote, setTypeoutNote] = useState<StigNote | null>(null);
  const [typeoutIdx, setTypeoutIdx] = useState(0);
  const [liveRun, setLiveRun] = useState<LiveRun | null>(null);
  const [liveToolEvents, setLiveToolEvents] = useState<RuntimeToolEvent[]>([]);
  const [runTimeline, setRunTimeline] = useState<RunTimelineEntry[]>([]);
  const [runNeedsApproval, setRunNeedsApproval] = useState(false);
  const [runActionBusy, setRunActionBusy] = useState(false);
  const [runActionErr, setRunActionErr] = useState<string | null>(null);
  const [railTab, setRailTab] = useState<"agents" | "files">("agents");
  const chatInputRef = useRef<HTMLTextAreaElement>(null);
  const chatScrollRef = useRef<HTMLDivElement | null>(null);
  const pollingRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const runPollingRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const runtimeEventSourceRef = useRef<EventSource | null>(null);
  const runtimeSeenRef = useRef<Set<string>>(new Set());
  const timelineSeqRef = useRef(0);
  const timelineRunRef = useRef<string | null>(null);
  const timelineStatusRef = useRef<RunStatus | null>(null);
  const brailleTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const typeoutTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  /** Coalesce NDJSON token updates to one React commit per animation frame. */
  const streamAppendAccRef = useRef("");
  const streamAppendRafRef = useRef<number | null>(null);

  const resetStreamAppend = useCallback(() => {
    streamAppendAccRef.current = "";
    if (streamAppendRafRef.current != null) {
      cancelAnimationFrame(streamAppendRafRef.current);
      streamAppendRafRef.current = null;
    }
    setStreamAssistantBuf("");
  }, []);

  const flushStreamAppendNow = useCallback(() => {
    if (streamAppendRafRef.current != null) {
      cancelAnimationFrame(streamAppendRafRef.current);
      streamAppendRafRef.current = null;
    }
    setStreamAssistantBuf(streamAppendAccRef.current);
  }, []);

  const scheduleStreamAppend = useCallback((chunk: string) => {
    if (!chunk) return;
    streamAppendAccRef.current += chunk;
    if (streamAppendRafRef.current != null) return;
    streamAppendRafRef.current = requestAnimationFrame(() => {
      streamAppendRafRef.current = null;
      setStreamAssistantBuf(streamAppendAccRef.current);
    });
  }, []);

  const BRAILLE = "⣾⣽⣻⢿⡿⣟⣯⣷";

  useEffect(() => {
    if (!companyId) {
      setChannels({});
      return;
    }
    setChannels(loadChannels(companyId));
  }, [companyId]);

  useEffect(() => {
    if (!companyId) setRailTab("agents");
  }, [companyId]);

  // Braille spinner animation while agent is thinking
  useEffect(() => {
    if (!thinking) {
      if (brailleTimerRef.current) clearInterval(brailleTimerRef.current);
      return;
    }
    brailleTimerRef.current = setInterval(() => setBrailleFrame((f) => (f + 1) % 8), 80);
    return () => { if (brailleTimerRef.current) clearInterval(brailleTimerRef.current); };
  }, [thinking]);

  // Typeout animation — live-streams agent reply character by character
  useEffect(() => {
    if (!typeoutNote) return;
    setTypeoutIdx(0);
    typeoutTimerRef.current = setInterval(() => {
      setTypeoutIdx((i) => {
        const next = i + 4;
        if (next >= typeoutNote.text.length) {
          if (typeoutTimerRef.current) clearInterval(typeoutTimerRef.current);
          return typeoutNote.text.length;
        }
        return next;
      });
    }, 18);
    return () => { if (typeoutTimerRef.current) clearInterval(typeoutTimerRef.current); };
  }, [typeoutNote]);


  const rows = useMemo((): AgentRow[] => {
    const m = new Map<string, AgentRow>();
    for (const a of agents) {
      if (a.status === "terminated") continue;
      const name = a.name.trim();
      if (!name) continue;
      m.set(name, {
        persona: name,
        registryId: a.id,
        liveCount: 0,
        title: a.title?.trim() || null,
        role: a.role?.trim() || null,
      });
    }
    for (const t of tasks) {
      const id = (t.owner_persona ?? t.checked_out_by ?? "").trim();
      if (!id) continue;
      if (!m.has(id)) {
        m.set(id, { persona: id, registryId: null, liveCount: 0, title: null, role: null });
      }
      const row = m.get(id)!;
      if (t.checked_out_by || /progress|doing|active/i.test(t.state)) row.liveCount += 1;
    }
    return [...m.values()].sort((a, b) => a.persona.localeCompare(b.persona));
  }, [agents, tasks]);

  const focusedPersona = useMemo(() => {
    if (propertiesSelection?.kind === "agent") {
      return (propertiesSelection.name ?? propertiesSelection.id).trim();
    }
    return "";
  }, [propertiesSelection]);

  // Stop polling and clear spinner when persona changes
  useEffect(() => {
    if (pollingRef.current) clearInterval(pollingRef.current);
    if (runPollingRef.current) clearInterval(runPollingRef.current);
    setThinking(false);
    setTypeoutNote(null);
    setLiveRun(null);
    setLiveToolEvents([]);
    setRunTimeline([]);
    setRunNeedsApproval(false);
    runtimeSeenRef.current.clear();
    setRunActionErr(null);
    resetStreamAppend();
  }, [focusedPersona, resetStreamAppend]);

  /** Clear live token bubble once the tracked worker run reaches a terminal state. */
  useEffect(() => {
    const st = liveRun?.status;
    if (st === "success" || st === "error" || st === "cancelled") {
      resetStreamAppend();
    }
  }, [liveRun?.status, resetStreamAppend]);

  const pushRunTimeline = useCallback((entry: Omit<RunTimelineEntry, "seq" | "tsMs"> & { tsMs?: number }) => {
    const seq = ++timelineSeqRef.current;
    const tsMs = entry.tsMs ?? Date.now();
    setRunTimeline((prev) => [...prev, { ...entry, seq, tsMs }].slice(-140));
  }, []);

  useEffect(() => {
    const rid = liveRun?.runId ?? null;
    if (!rid) return;
    if (timelineRunRef.current !== rid) {
      timelineRunRef.current = rid;
      timelineSeqRef.current = 0;
      timelineStatusRef.current = liveRun?.status ?? "running";
      setRunTimeline([]);
      pushRunTimeline({
        runId: rid,
        phase: "run_start",
        message: `Run started (${liveRun?.skill ?? "agent-loop"}).`,
      });
      return;
    }
    if (liveRun?.status && timelineStatusRef.current && liveRun.status !== timelineStatusRef.current) {
      timelineStatusRef.current = liveRun.status;
      pushRunTimeline({
        runId: rid,
        phase: "run_status",
        message: `Run status -> ${liveRun.status}${liveRun.summary ? ` (${liveRun.summary.slice(0, 120)})` : ""}`,
      });
    }
  }, [liveRun?.runId, liveRun?.status, liveRun?.summary, liveRun?.skill, pushRunTimeline]);

  // Poll agent-run status when a skill run is in flight
  useEffect(() => {
    if (!liveRun || liveRun.status !== "running" || !companyId) return;
    if (runPollingRef.current) clearInterval(runPollingRef.current);
    let elapsed = 0;
    /** Worker-backed runs often stay `running` in DB longer than chat UX; this is UI poll patience only. */
    const MAX_MS = 900_000;
    const INTERVAL = 3_000;
    runPollingRef.current = setInterval(async () => {
      elapsed += INTERVAL;
      if (elapsed >= MAX_MS) {
        clearInterval(runPollingRef.current!);
        setLiveRun((r) =>
          r
            ? {
                ...r,
                status: "error",
                summary:
                  "UI stopped polling after 15m with run still `running`. The worker may still be active — check the task in Company OS; restart is usually not required.",
              }
            : null,
        );
        return;
      }
      try {
        const { run } = await getAgentRun(apiBase, companyId, liveRun.runId);
        const st = parseRunStatus(run?.status);
        const summary = (typeof run?.summary === "string" ? run.summary : null) ?? null;
        const runTaskId = (typeof run?.task_id === "string" ? run.task_id : null) ?? liveRun.taskId ?? null;
        const runMode = parseExecutionMode(run?.meta?.execution_mode);

        if (runTaskId) {
          const taskJson = await listCompanyTasks(apiBase, companyId);
          const task = (taskJson.tasks ?? []).find((t) => t.id === runTaskId);
            const taskRunStatus = (task?.run?.status ?? "").toLowerCase();
            const taskToolCalls = task?.run?.tool_calls ?? 0;
            const observedWorker = taskToolCalls > 0;

            if (observedWorker && runMode !== "worker") {
              await patchAgentRun(apiBase, companyId, liveRun.runId, {
                meta: { ...(run?.meta ?? {}), execution_mode: "worker" },
              });
            }

            if ((taskRunStatus === "success" || taskRunStatus === "error") && st === "running") {
              const finalMode = observedWorker ? "worker" : "llm_simulated";
              const finalSummary =
                summary ??
                (typeof task?.run?.log_tail === "string" && task.run.log_tail.trim()
                  ? task.run.log_tail.slice(-500)
                  : `Task runtime ${taskRunStatus} (${taskToolCalls} tool calls)`);
              await patchAgentRun(apiBase, companyId, liveRun.runId, {
                status: taskRunStatus,
                summary: finalSummary,
                finished_at: true,
                meta: { ...(run?.meta ?? {}), execution_mode: finalMode },
              });
              clearInterval(runPollingRef.current!);
              setLiveRun((r) =>
                r
                  ? {
                      ...r,
                      status: taskRunStatus as RunStatus,
                      summary: finalSummary,
                      taskId: runTaskId,
                      executionMode: finalMode,
                    }
                  : null,
              );
              return;
            }
        }
        if (st && st !== "running") {
          clearInterval(runPollingRef.current!);
          setLiveRun((r) =>
            r
              ? {
                  ...r,
                  status: st,
                  summary,
                  taskId: runTaskId,
                  executionMode: runMode,
                }
              : null,
          );
        } else if (st) {
          setLiveRun((r) =>
            r
              ? {
                  ...r,
                  status: st,
                  summary: summary ?? r.summary,
                  taskId: runTaskId,
                  executionMode: runMode,
                }
              : null,
          );
        }
      } catch {
        /* ignore */
      }
    }, INTERVAL);
    return () => { if (runPollingRef.current) clearInterval(runPollingRef.current); };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [liveRun?.runId, liveRun?.status, companyId, apiBase]);

  useEffect(() => {
    setRunNeedsApproval(false);
    setLiveToolEvents([]);
    runtimeSeenRef.current.clear();
  }, [liveRun?.runId]);

  // Stream runtime tool events (SSE) into live run strip and persist to run feedback timeline.
  useEffect(() => {
    if (!companyId || !liveRun?.runId || liveRun.status !== "running") return;
    if (runtimeEventSourceRef.current) {
      runtimeEventSourceRef.current.close();
      runtimeEventSourceRef.current = null;
    }
    const es = new EventSource(`${apiBase}/api/company/runtime/events/stream`);
    runtimeEventSourceRef.current = es;

    es.onmessage = (ev) => {
      const raw = (() => {
        try {
          return JSON.parse(ev.data) as unknown;
        } catch {
          return null;
        }
      })();
      const obj = asObject(raw);
      const parsed: RuntimeToolEvent | null = obj
        ? {
            event_type: typeof obj.event_type === "string" ? obj.event_type : undefined,
            task_key: typeof obj.task_key === "string" || obj.task_key === null ? obj.task_key : undefined,
            tool_name: typeof obj.tool_name === "string" || obj.tool_name === null ? obj.tool_name : undefined,
            call_id: typeof obj.call_id === "string" || obj.call_id === null ? obj.call_id : undefined,
            success: typeof obj.success === "boolean" ? obj.success : undefined,
            message: typeof obj.message === "string" ? obj.message : undefined,
            ts_ms: typeof obj.ts_ms === "number" ? obj.ts_ms : undefined,
          }
        : null;
      if (!parsed) return;
      const key = `${parsed.ts_ms ?? 0}:${parsed.call_id ?? ""}:${parsed.event_type ?? ""}:${parsed.message ?? ""}`;
      if (runtimeSeenRef.current.has(key)) return;
      runtimeSeenRef.current.add(key);

      setLiveToolEvents((prev) => [parsed!, ...prev].slice(0, 24));
      const eventPhase: RunTimelinePhase =
        parsed.event_type === "tool_start"
          ? "tool_start"
          : parsed.success === false || /error|fail|blocked|denied/i.test(parsed.message ?? "")
            ? "tool_error"
            : "tool_complete";
      pushRunTimeline({
        runId: liveRun.runId,
        phase: eventPhase,
        toolName: parsed.tool_name ?? "tool",
        callId: parsed.call_id ?? null,
        tsMs: parsed.ts_ms,
        message: `${parsed.tool_name ?? "tool"}${parsed.call_id ? ` (${parsed.call_id})` : ""}${parsed.message ? ` — ${parsed.message}` : ""}`,
      });
      const tool = parsed.tool_name ?? "tool";
      const kind =
        parsed.event_type === "tool_start"
          ? "ToolStart"
          : parsed.success === false || /error|fail|blocked|denied/i.test(parsed.message ?? "")
            ? "ToolError"
            : "ToolComplete";
      const body = `[${kind}] ${tool}${parsed.call_id ? ` (${parsed.call_id})` : ""}: ${parsed.message ?? ""}`.trim();

      void (async () => {
        try {
          await postRunFeedback(apiBase, companyId, liveRun.runId, {
            actor: "runtime",
            kind: "comment",
            body,
            step_external_id: parsed.call_id ?? undefined,
          });
          const blocked = /blocked|approval|paused_approval|paused_auth|denied by approval|auth required|unauthorized|missing key|credentials/i.test(parsed.message ?? "");
          if (blocked && liveRun.taskId) {
            setRunNeedsApproval(true);
            const approvalKeyMatch = /approval required for [`'"]([^`'"]+)[`'"]/i.exec(parsed.message ?? "");
            const approvalKey = approvalKeyMatch?.[1] ?? null;
            const executionIdMatch = /execution[_\s-]?id[:=\s]+([0-9a-f-]{36})/i.exec(parsed.message ?? "");
            const executionId = executionIdMatch?.[1] ?? null;
            const pausedAuth = /paused_auth|auth required|unauthorized|missing key|credentials/i.test(parsed.message ?? "");
            const currentLoop = parseRunLoopState(liveRun?.status === "running" ? "running" : null) ?? "running";
            const nextLoop = pausedAuth ? "paused_auth" : "paused_approval";
            if (canTransitionRunLoopState(currentLoop, nextLoop)) {
              await patchAgentRun(apiBase, companyId, liveRun.runId, {
                summary: `${pausedAuth ? "Paused for auth" : "Paused for approval"}: ${tool}`,
                meta: {
                  execution_mode: "pending",
                  loop_state: nextLoop,
                  needs_human: true,
                  pending_approval_checkpoint: {
                    tool_name: tool,
                    call_id: parsed.call_id ?? null,
                    message: parsed.message ?? "",
                    approval_key: approvalKey,
                    execution_id: executionId,
                    kind: pausedAuth ? "auth" : "approval",
                    ts_ms: parsed.ts_ms ?? Date.now(),
                  },
                },
              });
            }
            pushRunTimeline({
              runId: liveRun.runId,
              phase: "checkpoint",
              toolName: tool,
              callId: parsed.call_id ?? null,
              tsMs: parsed.ts_ms,
              message: `Paused for approval checkpoint on ${tool}${parsed.call_id ? ` (${parsed.call_id})` : ""}.`,
            });
            await markTaskRequiresHuman(apiBase, liveRun.taskId, {
              requires_human: true,
              actor: "runtime",
              reason: `Tool gate blocked: ${tool}`,
            });
            await postRunFeedback(apiBase, companyId, liveRun.runId, {
              actor: "runtime",
              kind: "blocker",
              body: `Approval required for blocked tool action: ${tool}`,
              step_external_id: parsed.call_id ?? undefined,
            });
          }
        } catch {
          /* best effort */
        }
      })();
    };
    es.onerror = () => {
      // Browser auto-reconnects EventSource by default.
    };
    return () => {
      es.close();
      if (runtimeEventSourceRef.current === es) runtimeEventSourceRef.current = null;
    };
  }, [apiBase, companyId, liveRun?.runId, liveRun?.status, liveRun?.taskId, pushRunTimeline]);

  const selectedRow = useMemo(
    () => (focusedPersona ? rows.find((r) => r.persona === focusedPersona) : undefined),
    [rows, focusedPersona],
  );
  const selectedTask =
    propertiesSelection?.kind === "task"
      ? tasks.find((task) => task.id === propertiesSelection.id) ?? null
      : null;

  const buildIssue = useMutation({
    mutationFn: async (task: HsmTaskRow) => {
      const created = await createCompanyTask(apiBase, companyId!, {
        title: buildIssueTitleFromPlan(task.title),
        specification: buildIssueSpecFromPlan(task.specification),
        parent_task_id: task.id,
      });
      return { task: { id: created.taskId } };
    },
    onSuccess: async (data, task) => {
      await qc.invalidateQueries({ queryKey: ["hsm", "tasks", apiBase, companyId] });
      await qc.invalidateQueries({ queryKey: ["hsm", "intelligence", apiBase, companyId] });
      const createdId = data.task?.id?.trim();
      setPropertiesSelection({
        kind: "task",
        id: createdId || task.id,
        title: createdId
          ? `${buildIssueTitleFromPlan(task.title)}`
          : `${buildIssueTitleFromPlan(task.title)} created from ${task.title}`,
      });
    },
  });

  const workingPersonas = useMemo(() => {
    const s = new Set<string>();
    for (const r of rows) {
      if (isPersonaWorking(tasks, r.persona)) s.add(r.persona);
    }
    return s;
  }, [rows, tasks]);

  const selectPersona = useCallback(
    (row: AgentRow) => {
      setSendErr(null);
      setPropertiesSelection({
        kind: "agent",
        id: row.registryId ?? row.persona,
        name: row.persona,
      });
    },
    [setPropertiesSelection],
  );

  const clearAgent = useCallback(() => {
    setSendErr(null);
    setDraft("");
    if (propertiesSelection?.kind === "agent") {
      setPropertiesSelection(null);
    }
  }, [propertiesSelection, setPropertiesSelection]);

  const persistNotes = useCallback(
    (persona: string, taskId: string, notes: StigNote[]) => {
      if (!companyId) return;
      saveChannel(companyId, persona, { taskId, notes });
      setChannels((prev) => ({ ...prev, [persona]: { taskId, notes } }));
    },
    [companyId],
  );

  const startPollingForReply = useCallback(
    (taskId: string, registryId: string | null, knownKeys: Set<string>, persona: string) => {
      if (pollingRef.current) clearInterval(pollingRef.current);
      let elapsed = 0;
      const MAX_MS = 120_000;
      const INTERVAL = 2_000;

      pollingRef.current = setInterval(async () => {
        elapsed += INTERVAL;
        if (elapsed >= MAX_MS) {
          clearInterval(pollingRef.current!);
          setThinking(false);
          return;
        }
        if (!registryId || !companyId) return;
        try {
          const r = await fetch(
            `${apiBase}/api/company/companies/${companyId}/agents/${registryId}/operator-thread`,
          );
          if (!r.ok) return;
          const raw = await r.json().catch(() => ({}));
          const j = asObject(raw);
          const flat = asArray(j?.notes_flat).map((entry) => {
            const o = asObject(entry);
            const note = asObject(o?.note);
            return {
              task_id: typeof o?.task_id === "string" ? o.task_id : "",
              note: {
                at: typeof note?.at === "string" ? note.at : "",
                actor: typeof note?.actor === "string" ? note.actor : "operator",
                text: typeof note?.text === "string" ? note.text : "",
              } as StigNote,
            };
          });
          const taskNotes = flat
            .filter((n) => n.task_id === taskId)
            .map((n) => n.note)
            .filter((n) => n?.text);

          const newAgentNotes = taskNotes.filter(
            (n) => n.actor !== "operator" && !knownKeys.has(`${n.at}::${n.text.slice(0, 40)}`),
          );

          if (newAgentNotes.length > 0) {
            clearInterval(pollingRef.current!);
            setThinking(false);
            // Persist full updated notes list
            persistNotes(persona, taskId, taskNotes);
            // Kick off typeout for the last agent reply
            const last = newAgentNotes[newAgentNotes.length - 1];
            setTypeoutNote(last);
          }
        } catch {
          /* ignore transient polling errors */
        }
      }, INTERVAL);
    },
    [apiBase, companyId, persistNotes],
  );

  const sendHandoff = useCallback(async () => {
    if (!companyId || !focusedPersona) return;
    const text = draft.trim();
    if (!text) {
      setSendErr("Message required.");
      return;
    }
    setSendErr(null);
    setSending(true);
    if (pollingRef.current) clearInterval(pollingRef.current);
    setThinking(false);
    setTypeoutNote(null);
    try {
      let taskId =
        channels[focusedPersona]?.taskId ?? findBestTaskForPersona(tasks, focusedPersona) ?? null;

      // Step 1: Save the operator note (or create task)
      let notesAfterSend: StigNote[] = [];
      if (taskId) {
        const j = await postTaskStigmergicNote(apiBase, taskId, { text, actor: "operator" });
        notesAfterSend = parseNotesFromResponse(j.context_notes);
        persistNotes(focusedPersona, taskId, notesAfterSend);
      } else {
        const j = await createCompanyTask(apiBase, companyId, {
          title: `Operator · ${focusedPersona}`,
          specification: text,
          owner_persona: focusedPersona,
        });
        const newId = j.taskId;
        if (!newId) throw new Error("Created task missing id");
        taskId = newId;
        const now = new Date().toISOString();
        notesAfterSend = [{ at: now, actor: "operator", text }];
        persistNotes(focusedPersona, newId, notesAfterSend);
      }

      setDraft("");

      // Step 2: NDJSON stream — runtime/tool events + model token deltas (same-origin for Electron reliability)
      setThinking(true);
      setSending(false);
      resetStreamAppend();
      let hadStreamedTokens = false;

      const streamUrl = getAgentChatReplyStreamUrl();

      const ingestRuntimeFromPayload = (raw: unknown) => {
        const obj = asObject(raw);
        const parsed: RuntimeToolEvent | null = obj
          ? {
              event_type: typeof obj.event_type === "string" ? obj.event_type : undefined,
              task_key: typeof obj.task_key === "string" || obj.task_key === null ? obj.task_key : undefined,
              tool_name: typeof obj.tool_name === "string" || obj.tool_name === null ? obj.tool_name : undefined,
              call_id: typeof obj.call_id === "string" || obj.call_id === null ? obj.call_id : undefined,
              success: typeof obj.success === "boolean" ? obj.success : undefined,
              message: typeof obj.message === "string" ? obj.message : undefined,
              ts_ms: typeof obj.ts_ms === "number" ? obj.ts_ms : undefined,
            }
          : null;
        if (!parsed) return;
        const key = `${parsed.ts_ms ?? 0}:${parsed.call_id ?? ""}:${parsed.event_type ?? ""}:${parsed.message ?? ""}`;
        if (runtimeSeenRef.current.has(key)) return;
        runtimeSeenRef.current.add(key);
        setLiveToolEvents((prev) => [parsed, ...prev].slice(0, 24));
        const rid = liveRun?.runId;
        if (rid) {
          const eventPhase: RunTimelinePhase =
            parsed.event_type === "tool_start"
              ? "tool_start"
              : parsed.success === false || /error|fail|blocked|denied/i.test(parsed.message ?? "")
                ? "tool_error"
                : "tool_complete";
          pushRunTimeline({
            runId: rid,
            phase: eventPhase,
            toolName: parsed.tool_name ?? "tool",
            callId: parsed.call_id ?? null,
            tsMs: parsed.ts_ms,
            message: `${parsed.tool_name ?? "tool"}${parsed.call_id ? ` (${parsed.call_id})` : ""}${parsed.message ? ` — ${parsed.message}` : ""}`,
          });
        }
      };

      const replyRes = await fetch(streamUrl, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          taskId,
          persona: focusedPersona,
          companyId,
          title: selectedRow?.title,
          role: selectedRow?.role,
          notes: notesAfterSend,
        }),
      });

      let replyJ: {
        ok: boolean;
        reply?: string;
        at?: string;
        context_notes?: unknown;
        error?: string;
        run_id?: string;
        skill?: string;
        status?: string;
        execution_mode?: string;
        finalized?: boolean;
      } = { ok: false };

      if (!replyRes.ok || !replyRes.body) {
        const errText = await replyRes.text().catch(() => replyRes.statusText);
        setThinking(false);
        setSendErr(errText || `Chat stream failed (${replyRes.status})`);
        await refreshWorkspace();
        return;
      }

      const reader = replyRes.body.getReader();
      const dec = new TextDecoder();
      let lineBuf = "";
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        lineBuf += dec.decode(value, { stream: true });
        for (;;) {
          const nl = lineBuf.indexOf("\n");
          if (nl < 0) break;
          const line = lineBuf.slice(0, nl).trim();
          lineBuf = lineBuf.slice(nl + 1);
          if (!line) continue;
          let ev: Record<string, unknown>;
          try {
            ev = JSON.parse(line) as Record<string, unknown>;
          } catch {
            continue;
          }
          const t = typeof ev.type === "string" ? ev.type : "";
          if (t === "runtime" && ev.payload) {
            ingestRuntimeFromPayload(ev.payload);
          } else if (t === "stream_event") {
            const eff = extractAnthropicStreamTextEffect(ev.event);
            if (eff === "reset") {
              hadStreamedTokens = true;
              resetStreamAppend();
            } else if (eff && "append" in eff && eff.append.length > 0) {
              hadStreamedTokens = true;
              scheduleStreamAppend(eff.append);
            }
          } else if (t === "delta" && typeof ev.text === "string") {
            hadStreamedTokens = true;
            scheduleStreamAppend(ev.text);
          } else if (t === "error") {
            replyJ = {
              ok: false,
              error: typeof ev.message === "string" ? ev.message : "Stream error",
              run_id: typeof ev.run_id === "string" ? ev.run_id : undefined,
            };
          } else if (t === "done") {
            replyJ = {
              ok: ev.ok === true,
              reply: typeof ev.reply === "string" ? ev.reply : undefined,
              at: typeof ev.at === "string" ? ev.at : undefined,
              context_notes: ev.context_notes,
              run_id: typeof ev.run_id === "string" ? ev.run_id : undefined,
              skill: typeof ev.skill === "string" ? ev.skill : undefined,
              status: typeof ev.status === "string" ? ev.status : undefined,
              execution_mode: typeof ev.execution_mode === "string" ? ev.execution_mode : undefined,
              finalized: ev.finalized === true,
            };
          }
        }
      }

      flushStreamAppendNow();

      if (replyJ.run_id && companyId) {
        setLiveRun({
          runId: replyJ.run_id,
          skill: replyJ.skill ?? "worker-dispatch",
          status: "running",
          summary: null,
        });
        const rid = replyJ.run_id;
        void (async () => {
          try {
            const data = await getAgentRun(apiBase, companyId, rid);
            const run = data.run;
            setLiveRun((prev) =>
              prev?.runId === rid
                ? {
                    ...prev,
                    status: parseRunStatus(run?.status),
                    summary: typeof run?.summary === "string" ? run.summary : null,
                    taskId: typeof run?.task_id === "string" ? run.task_id : null,
                    executionMode: parseExecutionMode(run?.meta?.execution_mode),
                  }
              : prev,
            );
          } catch {
            /* ignore */
          }
        })();
        const knownKeys = new Set(notesAfterSend.map((n) => `${n.at}::${n.text.slice(0, 40)}`));
        startPollingForReply(taskId, selectedRow?.registryId ?? null, knownKeys, focusedPersona);
      } else {
        setThinking(false);
      }

      const keepStreamBubble = hadStreamedTokens && Boolean(replyJ.run_id);
      if (!keepStreamBubble) {
        resetStreamAppend();
      }
      if (!replyJ.ok || replyJ.error) {
        setThinking(false);
        setSendErr(replyJ.error ?? "Agent did not reply");
      } else if (replyJ.reply) {
        const agentNote: StigNote = {
          at: replyJ.at ?? new Date().toISOString(),
          actor: focusedPersona,
          text: replyJ.reply,
        };
        const fullNotes = parseNotesFromResponse(replyJ.context_notes);
        const notesToPersist = fullNotes.length > 0 ? fullNotes : [...notesAfterSend, agentNote];
        persistNotes(focusedPersona, taskId, notesToPersist);
        if (hadStreamedTokens) {
          setTypeoutNote(null);
        } else {
          setTypeoutNote(agentNote);
        }
        setThinking(false);
      } else if (replyJ.ok) {
        /* Worker dispatch: `done` often has run_id but no reply text; companion stream fills the bubble. */
        setThinking(false);
      }

      await refreshWorkspace();
      if (propertiesSelection?.kind === "agent" && companyId) {
        void qc.invalidateQueries({
          queryKey: ["hsm", "operator-thread", apiBase, companyId, propertiesSelection.id],
        });
      }
    } catch (e) {
      setThinking(false);
      const msg = e instanceof Error ? e.message : String(e);
      const low = msg.toLowerCase();
      setSendErr(
        low.includes("failed to fetch") || low.includes("load failed")
          ? `${msg} — Network/CORS/offline: task APIs use NEXT_PUBLIC_API_BASE (Rust) when set; the chat stream is Next-only at /api/agent-chat-reply/stream on this page’s origin. Run the company-console app (e.g. next dev or the desktop UI port), or set NEXT_PUBLIC_AGENT_CHAT_STREAM_URL to the full stream URL if the UI is opened from elsewhere.`
          : msg,
      );
    } finally {
      setSending(false);
    }
  }, [
    apiBase,
    channels,
    companyId,
    draft,
    flushStreamAppendNow,
    focusedPersona,
    persistNotes,
    propertiesSelection,
    qc,
    refreshWorkspace,
    resetStreamAppend,
    scheduleStreamAppend,
    startPollingForReply,
    selectedRow,
    tasks,
    pushRunTimeline,
  ]);

  const onRefreshList = useCallback(() => {
    if (!companyId) return;
    void qc.invalidateQueries({ queryKey: ["hsm", "agents", apiBase, companyId] });
    void qc.invalidateQueries({ queryKey: ["hsm", "tasks", apiBase, companyId] });
    void refreshWorkspace();
  }, [apiBase, companyId, qc, refreshWorkspace]);

  const escalateRunToHuman = useCallback(async () => {
    if (!companyId || !liveRun?.taskId) return;
    setRunActionBusy(true);
    setRunActionErr(null);
    try {
      await markTaskRequiresHuman(apiBase, liveRun.taskId, {
        requires_human: true,
        actor: "operator",
        reason: `Escalated from run ${liveRun.runId} (${liveRun.skill})`,
      });
      await refreshWorkspace();
      void qc.invalidateQueries({ queryKey: ["hsm", "tasks", apiBase, companyId] });
    } catch (e) {
      setRunActionErr(e instanceof Error ? e.message : String(e));
    } finally {
      setRunActionBusy(false);
    }
  }, [apiBase, companyId, liveRun?.runId, liveRun?.skill, liveRun?.taskId, qc, refreshWorkspace]);

  const promoteRunFollowupTask = useCallback(async () => {
    if (!companyId || !liveRun) return;
    setRunActionBusy(true);
    setRunActionErr(null);
    try {
      const feedback = await postRunFeedback(apiBase, companyId, liveRun.runId, {
        actor: "operator",
        kind: "blocker",
        body: `Follow-up requested by operator for skill ${liveRun.skill}.`,
      });
      const eventId = feedback.eventId;
      if (!eventId) throw new Error("Feedback event id missing");

      const promoteJ = await promoteRunFeedbackToTask(
        apiBase,
        companyId,
        liveRun.runId,
        eventId,
        {
          title: `Follow-up · ${liveRun.skill}`,
          specification: liveRun.summary ?? `Review run ${liveRun.runId} and complete any remaining work.`,
          owner_persona: focusedPersona || undefined,
        },
      );
      await refreshWorkspace();
      void qc.invalidateQueries({ queryKey: ["hsm", "tasks", apiBase, companyId] });
      const tid = promoteJ.taskId;
      if (tid) {
        setPropertiesSelection({
          kind: "task",
          id: tid,
          title: promoteJ.taskTitle ?? `Follow-up · ${liveRun.skill}`,
        });
      }
    } catch (e) {
      setRunActionErr(e instanceof Error ? e.message : String(e));
    } finally {
      setRunActionBusy(false);
    }
  }, [
    apiBase,
    companyId,
    focusedPersona,
    liveRun,
    qc,
    refreshWorkspace,
    setPropertiesSelection,
  ]);

  const resumeBlockedRun = useCallback(async () => {
    if (!companyId || !liveRun?.runId || !liveRun.taskId || !focusedPersona) return;
    setRunActionBusy(true);
    setRunActionErr(null);
    try {
      const j = await callResumeRun({
        companyId,
        runId: liveRun.runId,
        taskId: liveRun.taskId,
        persona: focusedPersona,
      });
      if (j.run_id) {
        pushRunTimeline({
          runId: liveRun.runId,
          phase: "resume",
          message: `Resume requested; continuing as run ${j.run_id}.`,
        });
        setLiveRun((prev) =>
          prev
            ? {
                ...prev,
                runId: j.run_id!,
                status: parseRunStatus(j.status),
                executionMode: parseExecutionMode(j.execution_mode),
                summary: "Resumed from approval checkpoint.",
              }
            : prev,
        );
      }
      setRunNeedsApproval(false);
      setLiveToolEvents([]);
      await refreshWorkspace();
      void qc.invalidateQueries({ queryKey: ["hsm", "tasks", apiBase, companyId] });
    } catch (e) {
      setRunActionErr(e instanceof Error ? e.message : String(e));
    } finally {
      setRunActionBusy(false);
    }
  }, [apiBase, companyId, focusedPersona, liveRun?.runId, liveRun?.taskId, pushRunTimeline, qc, refreshWorkspace]);

  const threadNotes = focusedPersona ? channels[focusedPersona]?.notes ?? [] : [];
  const visibleNotes = threadNotes.slice(-24);
  const showChat = focusedPersona.length > 0;
  const humanLabel = selectedRow?.title?.trim() || selectedRow?.role?.trim() || focusedPersona;
  const transcriptItems = useMemo((): ChatTranscriptItem[] => {
    const notes = visibleNotes.map((note, i) => ({
      kind: "note" as const,
      key: `note:${note.at}:${i}`,
      note,
      typing: typeoutNote !== null && note.at === typeoutNote.at && note.actor === typeoutNote.actor,
      sortTs: isoToMs(note.at),
    }));
    const tools = [...liveToolEvents]
      .reverse()
      .map((event, i) => ({
        kind: "tool" as const,
        key: `tool:${event.ts_ms ?? 0}:${event.call_id ?? "na"}:${i}`,
        event,
        sortTs: event.ts_ms ?? Date.now(),
      }));
    const items: Array<ChatTranscriptItem & { sortTs: number }> = [...notes, ...tools];
    items.sort((a, b) => a.sortTs - b.sortTs || a.key.localeCompare(b.key));
    const out: ChatTranscriptItem[] = items.map(({ sortTs: _sortTs, ...item }) => item);
    if (streamAssistantBuf) {
      out.push({
        kind: "assistant_partial",
        key: "assistant_partial:live",
        text: streamAssistantBuf,
      });
    }
    if (thinking) {
      out.push({
        kind: "status",
        key: `status:${liveRun?.runId ?? "thinking"}`,
        text:
          liveRun?.status === "running"
            ? `${focusedPersona} is running tools and composing a reply…`
            : `${focusedPersona} is thinking…`,
      });
    }
    return out;
  }, [
    focusedPersona,
    liveRun?.runId,
    liveRun?.status,
    liveToolEvents,
    streamAssistantBuf,
    thinking,
    typeoutNote,
    visibleNotes,
  ]);

  useEffect(() => {
    const el = chatScrollRef.current;
    if (!el) return;
    const id = requestAnimationFrame(() => {
      el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
    });
    return () => cancelAnimationFrame(id);
  }, [transcriptItems, typeoutIdx]);

  useEffect(() => {
    if (!showChat || !focusedPersona) return;
    const id = requestAnimationFrame(() => {
      chatInputRef.current?.focus();
    });
    return () => cancelAnimationFrame(id);
  }, [showChat, focusedPersona]);

  const headerBar = (
    <div className="flex shrink-0 flex-col gap-2 border-b border-[#222222] px-3 py-2">
      <div className="flex items-center justify-between gap-2">
        <div className="flex rounded-md border border-[#333333] bg-black p-0.5">
          <button
            type="button"
            title="Agents and chat"
            onClick={() => setRailTab("agents")}
            className={cn(
              "rounded px-2.5 py-1 font-mono text-[10px] uppercase tracking-wide transition-colors",
              railTab === "agents" ? "bg-white/[0.1] text-[#e8e8e8]" : "text-[#666666] hover:text-[#999999]",
            )}
          >
            Agents
          </button>
          <button
            type="button"
            title="Workspace files under company hsmii_home"
            onClick={() => setRailTab("files")}
            className={cn(
              "rounded px-2.5 py-1 font-mono text-[10px] uppercase tracking-wide transition-colors",
              railTab === "files" ? "bg-white/[0.1] text-[#e8e8e8]" : "text-[#666666] hover:text-[#999999]",
            )}
          >
            Files
          </button>
        </div>
        <button
          type="button"
          title="Refresh roster"
          onClick={() => onRefreshList()}
          className="flex size-8 items-center justify-center rounded-md text-[#666666] transition-colors hover:bg-white/[0.06] hover:text-[#e8e8e8]"
        >
          <RefreshCw className="size-3.5" strokeWidth={1.5} />
        </button>
      </div>
      {railTab === "agents" ? (
        <span className="font-sans text-[10px] font-medium uppercase tracking-[0.08em] text-[#777777]">
          Roster ({rows.length}) · say <span className="font-mono text-[#999999]">run [skill-slug]</span> to dispatch
        </span>
      ) : (
        <span className="font-sans text-[10px] font-medium uppercase tracking-[0.08em] text-[#777777]">
          Company workspace (hsmii_home)
        </span>
      )}
    </div>
  );

  const liveRunStrip =
    liveRun && companyId ? (
      <div className="shrink-0 border-b border-[#222222] bg-[#0d0d0d] px-3 py-2">
        <div className="flex items-start gap-2">
          <Badge
            variant="outline"
            className={cn(
              "shrink-0 border font-mono text-[9px] uppercase",
              liveRun.status === "running" && "border-amber-500/50 text-amber-200",
              liveRun.status === "success" && "border-emerald-500/40 text-emerald-200",
              liveRun.status === "error" && "border-red-500/40 text-red-200",
              liveRun.status === "cancelled" && "border-[#444444] text-[#999999]",
            )}
          >
            {liveRun.status}
          </Badge>
          <div className="min-w-0 flex-1">
            <p className="truncate font-mono text-[11px] text-[#e8e8e8]">Skill · {liveRun.skill}</p>
            <p className="mt-0.5 font-mono text-[9px] text-[#777777]">
              mode {liveRun.executionMode ?? "unknown"}
              {liveRun.taskId ? ` · task ${liveRun.taskId.slice(0, 8)}…` : ""}
            </p>
            {liveRun.summary ? (
              <p className="mt-0.5 line-clamp-2 font-mono text-[9px] text-[#777777]">{liveRun.summary}</p>
            ) : null}
          </div>
          <button
            type="button"
            title="Dismiss"
            className="flex size-7 shrink-0 items-center justify-center rounded-md text-[#666666] hover:bg-white/[0.06] hover:text-[#e8e8e8]"
            onClick={() => setLiveRun(null)}
          >
            <X className="size-3.5" strokeWidth={1.5} />
          </button>
        </div>
        {(liveRun.status === "error" || liveRun.status === "cancelled" || liveRun.status === "success") &&
        liveRun.taskId ? (
          <div className="mt-2 flex flex-wrap items-center gap-2">
            <Button
              variant="outline"
              size="xs"
              className="border-amber-500/40 bg-transparent font-mono text-[10px] text-amber-200 hover:bg-amber-500/10"
              disabled={runActionBusy}
              onClick={() => void escalateRunToHuman()}
            >
              {runActionBusy ? "Working…" : "Needs Human"}
            </Button>
            <Button
              variant="outline"
              size="xs"
              className="border-[#3b82f6]/40 bg-transparent font-mono text-[10px] text-[#8cc2ff] hover:bg-[#3b82f6]/10"
              disabled={runActionBusy}
              onClick={() => void promoteRunFollowupTask()}
            >
              {runActionBusy ? "Working…" : "Promote Follow-up"}
            </Button>
          </div>
        ) : null}
        {runActionErr ? (
          <p className="mt-2 font-mono text-[10px] uppercase tracking-wide text-[#D4A843]">[ERROR: {runActionErr}]</p>
        ) : null}
        {runNeedsApproval ? (
          <div className="mt-2 rounded border border-amber-500/30 bg-amber-500/5 px-2 py-1.5">
            <p className="font-mono text-[10px] text-amber-200">Blocked tool action queued for operator approval.</p>
            <div className="mt-1 flex flex-wrap items-center gap-2">
              <Link href="/workspace/approvals" className="font-mono text-[10px] text-amber-100 underline underline-offset-2">
                Open approvals inbox
              </Link>
              <Button
                variant="outline"
                size="xs"
                className="border-emerald-500/40 bg-transparent font-mono text-[10px] text-emerald-200 hover:bg-emerald-500/10"
                disabled={runActionBusy}
                onClick={() => void resumeBlockedRun()}
              >
                {runActionBusy ? "Working…" : "Resume Run"}
              </Button>
            </div>
          </div>
        ) : null}
        {liveToolEvents.length > 0 ? (
          <div className="mt-2 max-h-32 overflow-y-auto rounded border border-[#222222] bg-black/40 p-1.5">
            {liveToolEvents.map((e, i) => {
              const isErr = e.success === false || /error|fail|blocked|denied/i.test(e.message ?? "");
              const label = e.event_type === "tool_start" ? "ToolStart" : isErr ? "ToolError" : "ToolComplete";
              return (
                <p key={`${e.ts_ms ?? 0}-${e.call_id ?? "na"}-${i}`} className={cn("font-mono text-[9px]", isErr ? "text-red-200" : "text-[#9bb4d0]")}>
                  {label} · {e.tool_name ?? "tool"} {e.call_id ? `(${e.call_id})` : ""} {e.message ? `— ${e.message}` : ""}
                </p>
              );
            })}
          </div>
        ) : null}
        {runTimeline.length > 0 ? (
          <div className="mt-2 rounded border border-[#2b2b2b] bg-black/50 p-1.5">
            <p className="mb-1 font-mono text-[9px] uppercase tracking-wide text-[#8ea3bd]">Run timeline</p>
            <div className="max-h-36 space-y-0.5 overflow-y-auto">
              {runTimeline.map((evt) => {
                const tone =
                  evt.phase === "tool_error" || evt.phase === "checkpoint"
                    ? "text-amber-200"
                    : evt.phase === "run_status" && /error|cancelled/i.test(evt.message)
                      ? "text-red-200"
                      : "text-[#9bb4d0]";
                return (
                  <p key={`${evt.runId}-${evt.seq}`} className={cn("font-mono text-[9px]", tone)}>
                    #{evt.seq} [{evt.phase}] {evt.message}
                  </p>
                );
              })}
            </div>
          </div>
        ) : null}
      </div>
    ) : null;

  if (!companyId) {
    return (
      <div className="flex h-full flex-col bg-[#111111]">
        {headerBar}
        <p className="p-3 text-xs leading-relaxed text-[#999999]">Select a company in the header.</p>
      </div>
    );
  }

  return (
    <div className="flex h-full max-h-full min-h-0 w-full flex-col overflow-hidden bg-[#111111]">
      {headerBar}
      {liveRunStrip}

      {propertiesSelection?.kind === "task" ? (
        <div className="shrink-0 space-y-2 border-b border-[#222222] px-3 py-3">
          <p className="font-mono text-[10px] uppercase tracking-[0.06em] text-[#999999]">Task</p>
          <p className="line-clamp-2 font-sans text-sm font-medium text-[#e8e8e8]">
            {propertiesSelection.title ?? propertiesSelection.id}
          </p>
          <p className="break-all font-mono text-[10px] text-[#666666]">{propertiesSelection.id}</p>
          {selectedTask ? (
            <div className="flex flex-wrap items-center gap-2">
              {isPlanTask(selectedTask) ? (
                isDoneTask(selectedTask) ? (
                  <Button
                    variant="outline"
                    size="xs"
                    className="border-violet-500/50 bg-transparent font-mono text-[10px] text-violet-300 hover:bg-violet-500/10"
                    disabled={buildIssue.isPending}
                    onClick={() => buildIssue.mutate(selectedTask)}
                  >
                    {buildIssue.isPending ? "Building…" : "Build"}
                  </Button>
                ) : (
                  <Badge
                    variant="outline"
                    className="border-violet-500/40 bg-transparent font-mono text-[9px] text-violet-300"
                  >
                    plan
                  </Badge>
                )
              ) : null}
              {selectedTask.state ? (
                <Badge variant="outline" className="border-[#333333] bg-transparent font-mono text-[9px] text-[#999999]">
                  {selectedTask.state}
                </Badge>
              ) : null}
            </div>
          ) : null}
          {buildIssue.isError ? (
            <p className="text-xs text-red-300">
              {buildIssue.error instanceof Error ? buildIssue.error.message : String(buildIssue.error)}
            </p>
          ) : null}
          <div className="flex flex-wrap gap-2">
            <Button
              variant="outline"
              size="xs"
              className="border-[#333333] bg-transparent font-mono text-[11px] hover:bg-white/[0.06]"
              onClick={() => setPropertiesSelection(null)}
            >
              Clear
            </Button>
            <Button
              variant="outline"
              size="xs"
              asChild
              className="border-[#333333] bg-transparent font-mono text-[11px] hover:bg-white/[0.06]"
            >
              <Link href="/workspace/issues">Issues</Link>
            </Button>
          </div>
        </div>
      ) : null}

      {railTab === "files" ? (
        <WorkspaceRailFileBrowser apiBase={apiBase} companyId={companyId} />
      ) : !showChat ? (
        <>
          <div className="min-h-0 flex-1 overflow-y-auto px-2 py-2">
            {agentsLoading || tasksLoading ? (
              <p className="px-1 py-3 font-mono text-[10px] uppercase tracking-wide text-[#666666]">[LOADING…]</p>
            ) : rows.length === 0 ? (
              <p className="px-1 py-3 text-xs leading-relaxed text-[#666666]">
                No agents yet — add roster rows or assign tasks with{" "}
                <span className="font-mono text-[11px]">owner_persona</span>.
              </p>
            ) : (
              <ul className="space-y-0.5">
                {rows.map((row) => {
                  const active = focusedPersona === row.persona;
                  const busy = workingPersonas.has(row.persona);
                  return (
                    <li key={row.persona}>
                      <div
                        className={cn(
                          "group flex w-full items-stretch gap-0.5 rounded-md transition-colors duration-200 ease-out",
                          active ? "bg-white/[0.08]" : "hover:bg-white/[0.05]",
                        )}
                      >
                        <button
                          type="button"
                          title={`Chat with ${row.persona}`}
                          aria-label={
                            busy
                              ? `${row.persona}, active work in progress, open chat`
                              : `Chat with ${row.persona}`
                          }
                          onClick={() => selectPersona(row)}
                          className="flex min-w-0 flex-1 items-start gap-3 py-3 pl-2 pr-1 text-left"
                        >
                          <span
                            className={cn(
                              "mt-1 size-2 shrink-0 rounded-full bg-[#4A9E5C]",
                              busy && "nd-agent-status-dot--busy",
                            )}
                            aria-hidden
                          />
                          <div className="min-w-0 flex-1">
                            <span className="block font-mono text-[13px] leading-tight tracking-tight text-[#e8e8e8]">
                              {row.persona}
                            </span>
                            <p className="mt-1.5 line-clamp-2 font-sans text-[11px] leading-snug text-[#999999]">
                              {rowSubtitle(row)}
                            </p>
                          </div>
                        </button>
                        <div className="flex shrink-0 items-center self-center">
                          {row.registryId ? (
                            <Link
                              href={`/workspace/agents/${row.registryId}?tab=workspace`}
                              className={cn(
                                "flex size-10 items-center justify-center rounded-md transition-colors",
                                "text-[#666666] hover:bg-white/[0.06] hover:text-[#e8e8e8]",
                              )}
                              title="Open agent page (workspace files, memory, skills)"
                              aria-label={`Open full page for ${row.persona}`}
                              onClick={(e) => e.stopPropagation()}
                            >
                              <FolderOpen className="size-4" strokeWidth={1.5} aria-hidden />
                            </Link>
                          ) : null}
                          <button
                            type="button"
                            title={`Message ${row.persona}`}
                            aria-label={`Open message chat with ${row.persona}`}
                            onClick={() => selectPersona(row)}
                            className={cn(
                              "flex size-10 shrink-0 items-center justify-center rounded-md transition-colors",
                              active
                                ? "text-[#e8e8e8]"
                                : "text-[#666666] hover:bg-white/[0.06] hover:text-[#e8e8e8] group-hover:text-[#999999]",
                            )}
                          >
                            <MessageSquare className="size-4" strokeWidth={1.5} aria-hidden />
                          </button>
                        </div>
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
          <div className="shrink-0 border-t border-[#222222] px-3 py-3">
            <p className="text-center text-xs leading-relaxed text-[#666666]">
              Tap a row or the message icon to open chat. Use the folder icon to open the full agent page (files,
              memory, skills). Messages create or update tasks for that assignee.
            </p>
            <p className="mt-2 text-center font-mono text-[10px] uppercase tracking-[0.06em] text-[#555555]">
              Palette <kbd className="rounded border border-[#333333] bg-black px-1 text-[#999999]">⌘K</kbd>
            </p>
          </div>
        </>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
          <div className="shrink-0 border-b border-[#222222] px-3 pb-3 pt-2">
            <div className="flex items-center gap-2">
              <span
                className={cn(
                  "size-2 shrink-0 rounded-full bg-[#4A9E5C]",
                  workingPersonas.has(focusedPersona) && "nd-agent-status-dot--busy",
                )}
                aria-hidden
              />
              <Bot className="size-4 shrink-0 text-[#666666]" strokeWidth={1.5} />
              <p className="min-w-0 flex-1 truncate font-mono text-sm text-[#e8e8e8]">{focusedPersona}</p>
              <span
                className="hidden max-w-[9rem] truncate rounded border border-[#333333] px-2 py-1 font-mono text-[9px] font-medium uppercase tracking-[0.06em] text-[#e8e8e8] sm:inline-block"
                title={rolePillText(selectedRow)}
              >
                {rolePillText(selectedRow)}
              </span>
              <button
                type="button"
                title="Close"
                onClick={clearAgent}
                className="flex size-8 shrink-0 items-center justify-center rounded-md text-[#666666] hover:bg-white/[0.06] hover:text-[#e8e8e8]"
              >
                <X className="size-4" strokeWidth={1.5} />
              </button>
            </div>
            <p className="mt-2 pl-6 text-[11px] leading-relaxed text-[#666666]">
              {humanLabel} — messages become tasks assigned to this agent.
              {selectedRow?.registryId ? (
                <>
                  {" "}
                  <Link
                    href={`/workspace/agents/${selectedRow.registryId}?tab=workspace`}
                    className="font-medium text-[#e8e8e8] underline-offset-4 hover:underline"
                  >
                    Workspace files
                  </Link>
                  , memory, and skills live on the full agent page.
                </>
              ) : null}
            </p>
            <span
              className="mt-2 inline-block max-w-full truncate rounded border border-[#333333] px-2 py-1 font-mono text-[9px] font-medium uppercase tracking-[0.06em] text-[#e8e8e8] sm:hidden"
              title={rolePillText(selectedRow)}
            >
              {rolePillText(selectedRow)}
            </span>
          </div>

          <div ref={chatScrollRef} className="min-h-0 flex-1 overflow-y-auto px-3 py-4">
            {transcriptItems.length === 0 ? (
              <p className="px-1 text-center text-sm text-[#666666]">
                No messages yet. Send one to create a task for {focusedPersona}.
              </p>
            ) : (
              <div className="space-y-3">
                {transcriptItems.map((item) => {
                  if (item.kind === "note") {
                    const n = item.note;
                    return (
                      <div key={item.key} className="text-xs leading-snug">
                        <span className="font-mono text-[10px] uppercase tracking-wide text-[#666666]">
                          {n.actor}
                          {n.at ? ` · ${n.at.slice(0, 19)}` : ""}
                        </span>
                        <div className="mt-1">
                          {item.typing && typeoutNote ? (
                            <div className="nd-stream-assistant__body">
                              <div className="min-w-0 flex-1">
                                <WorkspaceMarkdownBody text={typeoutNote.text.slice(0, typeoutIdx)} />
                              </div>
                              {typeoutIdx < typeoutNote.text.length ? (
                                <span
                                  className="nd-stream-assistant__caret nd-stream-assistant__caret--pulse"
                                  aria-hidden
                                />
                              ) : null}
                            </div>
                          ) : (
                            <WorkspaceMarkdownBody text={n.text} />
                          )}
                        </div>
                      </div>
                    );
                  }
                  if (item.kind === "tool") {
                    const event = item.event;
                    const isErr =
                      event.success === false || /error|fail|blocked|denied/i.test(event.message ?? "");
                    return (
                      <div
                        key={item.key}
                        className={cn(
                          "rounded-md border px-3 py-2",
                          isErr
                            ? "border-amber-500/30 bg-amber-500/5"
                            : "border-[#223040] bg-[#0c1117]",
                        )}
                      >
                        <div className="flex items-center gap-2">
                          <span
                            className={cn(
                              "rounded border px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-wide",
                              isErr
                                ? "border-amber-500/40 text-amber-200"
                                : "border-[#31465f] text-[#8ea3bd]",
                            )}
                          >
                            {formatRuntimeEventLabel(event)}
                          </span>
                          <span className="font-mono text-[10px] text-[#9bb4d0]">
                            {event.tool_name ?? "tool"}
                            {event.call_id ? ` · ${event.call_id}` : ""}
                          </span>
                        </div>
                        {event.message ? (
                          <p className={cn("mt-1 whitespace-pre-wrap text-[11px]", isErr ? "text-amber-100" : "text-[#c8d4e3]")}>
                            {event.message}
                          </p>
                        ) : null}
                      </div>
                    );
                  }
                  if (item.kind === "assistant_partial") {
                    return (
                      <div key={item.key} className="nd-stream-assistant">
                        <div className="nd-stream-assistant__meta">
                          <span className="nd-stream-assistant__label">Streaming</span>
                          <span className="nd-stream-assistant__persona">{focusedPersona}</span>
                        </div>
                        <div className="nd-stream-assistant__body">
                          <div className="min-w-0 flex-1">
                            <WorkspaceMarkdownStreamBody
                              text={item.text}
                              className={cn(
                                "text-[#e8e8e8]",
                                "[&_a]:text-[#999999] hover:[&_a]:text-[#e8e8e8]",
                                "[&_blockquote]:border-[#333333] [&_blockquote]:text-[#999999]",
                                "[&_code]:text-[#e8e8e8]",
                              )}
                            />
                          </div>
                          <span
                            className="nd-stream-assistant__caret nd-stream-assistant__caret--pulse"
                            aria-hidden
                          />
                        </div>
                      </div>
                    );
                  }
                  return (
                    <div key={item.key} className="text-xs leading-snug">
                      <span className="font-mono text-[10px] uppercase tracking-wide text-[#555555]">
                        {focusedPersona}
                      </span>
                      <p className="mt-1 flex items-center gap-1.5 text-[#666666]">
                        <span className="font-mono text-sm text-[#4a9eff]">{BRAILLE[brailleFrame]}</span>
                        <span className="font-mono text-[10px] uppercase tracking-wide">{item.text}</span>
                      </p>
                    </div>
                  );
                })}
              </div>
            )}
          </div>

          <div className="shrink-0 border-t border-[#222222] bg-[#0a0a0a] p-3">
            {sendErr ? (
              <p className="mb-2 font-mono text-[10px] uppercase tracking-wide text-[#D4A843]">
                [ERROR: {sendErr}]
              </p>
            ) : null}
            <div className="flex gap-2">
              <Textarea
                ref={chatInputRef}
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key !== "Enter") return;
                  if (e.shiftKey) return;
                  if (e.nativeEvent.isComposing) return;
                  e.preventDefault();
                  void sendHandoff();
                }}
                placeholder={`Message ${focusedPersona}… (Enter send · Shift+Enter new line)`}
                rows={2}
                disabled={sending}
                className="min-h-[44px] flex-1 resize-none border-[#333333] bg-black font-sans text-sm text-[#e8e8e8] placeholder:text-[#555555] focus-visible:border-[#555555] focus-visible:ring-1 focus-visible:ring-[#444444]"
              />
              <Button
                type="button"
                variant="outline"
                size="icon"
                disabled={sending}
                title="Send"
                className="size-11 shrink-0 border-[#333333] bg-black text-[#e8e8e8] hover:bg-white/[0.08]"
                onClick={() => void sendHandoff()}
              >
                <Send className="size-4" strokeWidth={1.5} />
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
