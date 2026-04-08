"use client";

import Link from "next/link";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Bot, FolderOpen, MessageSquare, RefreshCw, Send, X } from "lucide-react";

import { Badge } from "@/app/components/ui/badge";
import { Button } from "@/app/components/ui/button";
import { Textarea } from "@/app/components/ui/textarea";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import type { HsmTaskRow } from "@/app/lib/hsm-api-types";
import { useCompanyAgents, useCompanyTasks } from "@/app/lib/hsm-queries";
import { companyOsUrl } from "@/app/lib/company-api-url";
import { cn } from "@/app/lib/utils";
import { buildIssueSpecFromPlan, buildIssueTitleFromPlan, isDoneTask, isPlanTask } from "@/app/lib/workspace-issue";

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
  const chatInputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (!companyId) {
      setChannels({});
      return;
    }
    setChannels(loadChannels(companyId));
  }, [companyId]);

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
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/tasks`), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          title: buildIssueTitleFromPlan(task.title),
          specification: buildIssueSpecFromPlan(task.specification),
          parent_task_id: task.id,
        }),
      });
      const j = (await r.json().catch(() => ({}))) as { error?: string; task?: { id?: string } };
      if (!r.ok) throw new Error(j.error ?? r.statusText);
      return j;
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

  const sendHandoff = useCallback(async () => {
    if (!companyId || !focusedPersona) return;
    const text = draft.trim();
    if (!text) {
      setSendErr("Message required.");
      return;
    }
    setSendErr(null);
    setSending(true);
    try {
      let taskId =
        channels[focusedPersona]?.taskId ?? findBestTaskForPersona(tasks, focusedPersona) ?? null;

      if (taskId) {
        const r = await fetch(`${apiBase}/api/company/tasks/${taskId}/stigmergic-note`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ text, actor: "operator" }),
        });
        const j = (await r.json().catch(() => ({}))) as {
          context_notes?: unknown;
          error?: string;
        };
        if (!r.ok) throw new Error(j.error ?? r.statusText);
        const notes = parseNotesFromResponse(j.context_notes);
        persistNotes(focusedPersona, taskId, notes);
      } else {
        const r = await fetch(`${apiBase}/api/company/companies/${companyId}/tasks`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            title: `Operator · ${focusedPersona}`,
            specification: text,
            owner_persona: focusedPersona,
          }),
        });
        const j = (await r.json().catch(() => ({}))) as {
          task?: { id?: string };
          error?: string;
        };
        if (!r.ok) throw new Error(j.error ?? r.statusText);
        const newId = j.task?.id;
        if (!newId) throw new Error("Created task missing id");
        const now = new Date().toISOString();
        const notes: StigNote[] = [{ at: now, actor: "operator", text }];
        persistNotes(focusedPersona, newId, notes);
      }

      setDraft("");
      await refreshWorkspace();
    } catch (e) {
      setSendErr(e instanceof Error ? e.message : String(e));
    } finally {
      setSending(false);
    }
  }, [
    apiBase,
    channels,
    companyId,
    draft,
    focusedPersona,
    persistNotes,
    refreshWorkspace,
    tasks,
  ]);

  const onRefreshList = useCallback(() => {
    if (!companyId) return;
    void qc.invalidateQueries({ queryKey: ["hsm", "agents", apiBase, companyId] });
    void qc.invalidateQueries({ queryKey: ["hsm", "tasks", apiBase, companyId] });
    void refreshWorkspace();
  }, [apiBase, companyId, qc, refreshWorkspace]);

  const threadNotes = focusedPersona ? channels[focusedPersona]?.notes ?? [] : [];
  const visibleNotes = threadNotes.slice(-24);
  const showChat = focusedPersona.length > 0;
  const humanLabel = selectedRow?.title?.trim() || selectedRow?.role?.trim() || focusedPersona;

  useEffect(() => {
    if (!showChat || !focusedPersona) return;
    const id = requestAnimationFrame(() => {
      chatInputRef.current?.focus();
    });
    return () => cancelAnimationFrame(id);
  }, [showChat, focusedPersona]);

  const headerBar = (
    <div className="flex shrink-0 items-center justify-between gap-2 border-b border-[#222222] px-3 py-3">
      <span className="font-sans text-[11px] font-medium uppercase tracking-[0.1em] text-[#999999]">
        Agents ({rows.length})
      </span>
      <button
        type="button"
        title="Refresh roster"
        onClick={() => onRefreshList()}
        className="flex size-8 items-center justify-center rounded-md text-[#666666] transition-colors hover:bg-white/[0.06] hover:text-[#e8e8e8]"
      >
        <RefreshCw className="size-3.5" strokeWidth={1.5} />
      </button>
    </div>
  );

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

      {!showChat ? (
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

          <div className="min-h-0 flex-1 overflow-y-auto px-3 py-4">
            {visibleNotes.length === 0 ? (
              <p className="px-1 text-center text-sm text-[#666666]">
                No messages yet. Send one to create a task for {focusedPersona}.
              </p>
            ) : (
              <div className="space-y-3">
                {visibleNotes.map((n, i) => (
                  <div key={`${n.at}-${i}`} className="text-xs leading-snug">
                    <span className="font-mono text-[10px] uppercase tracking-wide text-[#666666]">
                      {n.actor}
                      {n.at ? ` · ${n.at.slice(0, 19)}` : ""}
                    </span>
                    <p className="mt-1 whitespace-pre-wrap text-[#c8c8c8]">{n.text}</p>
                  </div>
                ))}
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
