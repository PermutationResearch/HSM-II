"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { Users, Send, RotateCcw, CheckCircle2, History, X } from "lucide-react";
import { Button } from "@/app/components/ui/button";
import { Textarea } from "@/app/components/ui/textarea";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { useCompanyAgents } from "@/app/lib/hsm-queries";
import { cn } from "@/app/lib/utils";
import type { HsmCompanyAgentRow } from "@/app/lib/hsm-api-types";

/* ─── Types ───────────────────────────────────────────────────────────── */

type AgentTurn = {
  agent: string;
  role: string;
  round: number;
  /** Accumulating live text */
  live: string;
  /** Finalised content */
  content: string | null;
};

type ConsensusTurn = {
  live: string;
  content: string | null;
};

/** One completed deliberation stored in session history */
type CouncilHistoryEntry = {
  at: string; // ISO timestamp
  query: string;
  turns: Array<{ agent: string; round: number; content: string }>;
  consensus: string;
};

type Status = "idle" | "running" | "done" | "error";

/* ─── History storage helpers ─────────────────────────────────────────── */

const COUNCIL_HISTORY_MAX_CHARS = 5_000;
const COUNCIL_HISTORY_KEEP_RECENT = 2;
const COUNCIL_HISTORY_MAX_ENTRIES = 20;

function historyStorageKey(companyId: string | null | undefined): string {
  return `council-history:${companyId ?? "default"}`;
}

function loadHistoryFromStorage(companyId: string | null | undefined): CouncilHistoryEntry[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = sessionStorage.getItem(historyStorageKey(companyId));
    if (!raw) return [];
    return JSON.parse(raw) as CouncilHistoryEntry[];
  } catch {
    return [];
  }
}

function persistHistory(companyId: string | null | undefined, entries: CouncilHistoryEntry[]): void {
  if (typeof window === "undefined") return;
  try {
    const trimmed = entries.slice(-COUNCIL_HISTORY_MAX_ENTRIES);
    sessionStorage.setItem(historyStorageKey(companyId), JSON.stringify(trimmed));
  } catch {
    /* storage quota — ignore */
  }
}

/**
 * Build a prior-context string from accumulated history to inject into the
 * council's system prompts. Compacts old entries when the total exceeds the
 * character budget, mirroring the agent-chat compaction pattern.
 */
function buildPriorContext(
  history: CouncilHistoryEntry[],
): { text: string; compacted: boolean } | null {
  if (history.length === 0) return null;

  const formatEntry = (e: CouncilHistoryEntry) => {
    const turnLines = e.turns
      .map((t) => `[${t.agent} r${t.round + 1}]: ${t.content.trim()}`)
      .join("\n");
    return `### Deliberation: "${e.query}" (${e.at.slice(0, 16).replace("T", " ")} UTC)\n${turnLines}\n\nResolution: ${e.consensus}`;
  };

  const formatted = history.map(formatEntry);
  const fullText = formatted.join("\n\n---\n\n");

  if (fullText.length <= COUNCIL_HISTORY_MAX_CHARS) {
    return { text: fullText, compacted: false };
  }

  // Compact older entries — keep the last KEEP_RECENT verbatim
  const recentEntries = history.slice(-COUNCIL_HISTORY_KEEP_RECENT);
  const olderEntries = history.slice(0, -COUNCIL_HISTORY_KEEP_RECENT);

  const olderSummary =
    olderEntries.length > 0
      ? `### Earlier deliberations (${olderEntries.length} compacted)\n` +
        olderEntries
          .map((e) => {
            const snippet = e.consensus.slice(0, 140).replace(/\n/g, " ");
            return `- [${e.at.slice(0, 10)}] "${e.query}" → ${snippet}${e.consensus.length > 140 ? "…" : ""}`;
          })
          .join("\n")
      : null;

  const parts: string[] = [];
  if (olderSummary) parts.push(olderSummary);
  parts.push(...recentEntries.map(formatEntry));

  return { text: parts.join("\n\n---\n\n"), compacted: true };
}

/* ─── Colour palette per agent index ──────────────────────────────────── */

const AGENT_COLOURS = [
  "text-[#4a9eff]",
  "text-[#a78bfa]",
  "text-[#34d399]",
  "text-[#fb923c]",
  "text-[#f472b6]",
  "text-[#38bdf8]",
  "text-[#facc15]",
  "text-[#a3e635]",
];

function agentColour(name: string, roster: string[]): string {
  const idx = roster.indexOf(name);
  return AGENT_COLOURS[idx >= 0 ? idx % AGENT_COLOURS.length : 0];
}

/* ─── Typing caret ─────────────────────────────────────────────────────── */

function Caret() {
  return (
    <span
      className="nd-stream-assistant__caret nd-stream-assistant__caret--pulse ml-0.5 inline-block"
      aria-hidden
    />
  );
}

/* ─── Single turn bubble ───────────────────────────────────────────────── */

function TurnBubble({
  turn,
  agentColourClass,
  isLive,
}: {
  turn: AgentTurn;
  agentColourClass: string;
  isLive: boolean;
}) {
  const text = turn.content ?? turn.live;
  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-baseline gap-2">
        <span className={cn("font-mono text-[10px] font-medium uppercase tracking-widest", agentColourClass)}>
          {turn.agent}
        </span>
        <span className="font-mono text-[9px] text-[#3a3a3a]">r{turn.round + 1}</span>
        {turn.role ? (
          <span className="font-mono text-[9px] uppercase tracking-wide text-[#444444]">{turn.role}</span>
        ) : null}
      </div>
      <p className="text-[13px] leading-relaxed text-[#c8c8c8] whitespace-pre-wrap">
        {text}
        {isLive ? <Caret /> : null}
      </p>
    </div>
  );
}

/* ─── Consensus box ────────────────────────────────────────────────────── */

function ConsensusBox({ consensus }: { consensus: ConsensusTurn }) {
  const text = consensus.content ?? consensus.live;
  const isLive = consensus.content === null;
  return (
    <div className="rounded border border-[#1e3a2a] bg-[#0a150e] p-3">
      <div className="mb-2 flex items-center gap-2">
        <CheckCircle2 className="size-3.5 text-[#34d399]" strokeWidth={1.5} />
        <span className="font-mono text-[10px] uppercase tracking-widest text-[#34d399]">Council resolution</span>
      </div>
      <p className="text-[13px] leading-relaxed text-[#d4d4d4] whitespace-pre-wrap">
        {text}
        {isLive ? <Caret /> : null}
      </p>
    </div>
  );
}

/* ─── History card (past deliberation, collapsed by default) ───────────── */

function HistoryCard({
  entry,
  index,
  rosterNames,
}: {
  entry: CouncilHistoryEntry;
  index: number;
  rosterNames: string[];
}) {
  const [expanded, setExpanded] = useState(false);
  const timeStr = entry.at.slice(0, 16).replace("T", " ");

  return (
    <div className="rounded border border-[#161616] bg-[#060606]">
      <button
        type="button"
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-start gap-2 px-3 py-2 text-left hover:bg-white/[0.02] transition-colors"
      >
        <span className="font-mono text-[9px] text-[#333333] shrink-0 mt-0.5">#{index + 1}</span>
        <div className="flex-1 min-w-0">
          <p className="font-mono text-[10px] text-[#555555] truncate">{entry.query}</p>
          <p className="font-mono text-[9px] text-[#2e2e2e] mt-0.5">
            {timeStr} UTC · {entry.turns.length} turn{entry.turns.length !== 1 ? "s" : ""}
          </p>
        </div>
        <span className="font-mono text-[9px] text-[#2a2a2a] shrink-0">{expanded ? "▲" : "▼"}</span>
      </button>
      {expanded ? (
        <div className="border-t border-[#0e0e0e] px-3 py-2 space-y-2">
          {entry.turns.map((t, i) => (
            <div key={i} className="flex gap-2">
              <span className={cn("font-mono text-[9px] uppercase shrink-0 mt-0.5", agentColour(t.agent, rosterNames))}>
                {t.agent}
              </span>
              <p className="text-[11px] text-[#666666] leading-relaxed whitespace-pre-wrap">{t.content}</p>
            </div>
          ))}
          <div className="mt-2 border-t border-[#0e0e0e] pt-2">
            <p className="font-mono text-[9px] uppercase text-[#2a6040] mb-1">Resolution</p>
            <p className="text-[11px] text-[#777777] whitespace-pre-wrap">{entry.consensus}</p>
          </div>
        </div>
      ) : null}
    </div>
  );
}

/* ─── Agent chip ───────────────────────────────────────────────────────── */

function AgentChip({
  agent,
  colourClass,
  active,
}: {
  agent: HsmCompanyAgentRow;
  colourClass: string;
  active: boolean;
}) {
  return (
    <div
      className={cn(
        "flex flex-col gap-0.5 rounded border px-2 py-1.5 transition-colors",
        active ? "border-[#2a2a2a] bg-[#111111]" : "border-[#1a1a1a] bg-[#080808] opacity-50",
      )}
    >
      <span className={cn("font-mono text-[10px] font-medium uppercase tracking-wide", colourClass)}>
        {agent.name}
      </span>
      {agent.title || agent.role ? (
        <span className="font-mono text-[9px] text-[#555555] leading-tight">
          {[agent.title, agent.role].filter(Boolean).join(" · ")}
        </span>
      ) : null}
    </div>
  );
}

/* ─── Main panel ───────────────────────────────────────────────────────── */

export function CouncilChamberPanel() {
  const { apiBase, companyId } = useWorkspace();
  const { data: agents = [], isLoading: agentsLoading } = useCompanyAgents(apiBase, companyId);

  const activeAgents = agents.filter((a) => a.status !== "terminated" && a.name.trim());
  const rosterNames = activeAgents.map((a) => a.name);

  const [query, setQuery] = useState("");
  const [rounds, setRounds] = useState(1);
  const [turns, setTurns] = useState<AgentTurn[]>([]);
  const [consensus, setConsensus] = useState<ConsensusTurn | null>(null);
  const [activeAgent, setActiveAgent] = useState<string | null>(null);
  const [status, setStatus] = useState<Status>("idle");
  const [error, setError] = useState<string | null>(null);
  const [sessionHistory, setSessionHistory] = useState<CouncilHistoryEntry[]>([]);
  const [historyCompacted, setHistoryCompacted] = useState(false);

  // Refs to track turns during streaming without stale-closure issues
  const completedTurnsRef = useRef<Array<{ agent: string; round: number; content: string }>>([]);
  const currentQueryRef = useRef<string>("");

  const scrollRef = useRef<HTMLDivElement>(null);
  const abortRef = useRef<AbortController | null>(null);

  // Load persisted history when companyId changes
  useEffect(() => {
    setSessionHistory(loadHistoryFromStorage(companyId));
    setHistoryCompacted(false);
  }, [companyId]);

  // Auto-scroll on new content
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const id = requestAnimationFrame(() => el.scrollTo({ top: el.scrollHeight, behavior: "smooth" }));
    return () => cancelAnimationFrame(id);
  }, [turns, consensus]);

  const clearHistory = useCallback(() => {
    setSessionHistory([]);
    setHistoryCompacted(false);
    persistHistory(companyId, []);
  }, [companyId]);

  /** Clear current deliberation state (not history) */
  const reset = useCallback(() => {
    abortRef.current?.abort();
    setTurns([]);
    setConsensus(null);
    setActiveAgent(null);
    setStatus("idle");
    setError(null);
    completedTurnsRef.current = [];
  }, []);

  const start = useCallback(async () => {
    const q = query.trim();
    if (!q || activeAgents.length === 0) return;
    reset();
    setStatus("running");
    currentQueryRef.current = q;
    completedTurnsRef.current = [];

    // Build prior context from accumulated history
    const priorCtx = buildPriorContext(sessionHistory);
    if (priorCtx?.compacted) setHistoryCompacted(true);

    const abort = new AbortController();
    abortRef.current = abort;

    try {
      const res = await fetch("/api/council/deliberate", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        signal: abort.signal,
        body: JSON.stringify({
          query: q,
          prior_context: priorCtx?.text ?? null,
          agents: activeAgents.map((a) => ({
            name: a.name,
            role: a.role ?? null,
            title: a.title ?? null,
            capabilities: a.capabilities ?? null,
          })),
          rounds,
        }),
      });

      if (!res.ok || !res.body) {
        const errText = await res.text().catch(() => res.statusText);
        throw new Error(errText);
      }

      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buf = "";

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buf += decoder.decode(value, { stream: true });
        const lines = buf.split("\n");
        buf = lines.pop() ?? "";

        for (const line of lines) {
          const trimmed = line.trim();
          if (!trimmed) continue;
          let ev: Record<string, unknown>;
          try {
            ev = JSON.parse(trimmed) as Record<string, unknown>;
          } catch {
            continue;
          }

          const type = ev.type as string;

          if (type === "turn_start") {
            const agent = ev.agent as string;
            const role = (ev.role as string) ?? "";
            const round = (ev.round as number) ?? 0;
            setActiveAgent(agent);
            setTurns((prev) => [
              ...prev,
              { agent, role, round, live: "", content: null },
            ]);
          } else if (type === "token") {
            const agent = ev.agent as string;
            const delta = (ev.delta as string) ?? "";
            if (agent === "council") {
              setConsensus((prev) =>
                prev ? { ...prev, live: prev.live + delta } : { live: delta, content: null },
              );
            } else {
              setTurns((prev) =>
                prev.map((t) =>
                  t.agent === agent && t.content === null
                    ? { ...t, live: t.live + delta }
                    : t,
                ),
              );
            }
          } else if (type === "turn_done") {
            const agent = ev.agent as string;
            const content = (ev.content as string) ?? "";
            const round = typeof ev.round === "number" ? ev.round : 0;
            setTurns((prev) =>
              prev.map((t) =>
                t.agent === agent && t.content === null ? { ...t, content } : t,
              ),
            );
            // Track for history entry (ref is safe to use in async context)
            completedTurnsRef.current.push({ agent, round, content });
          } else if (type === "consensus_start") {
            setActiveAgent("council");
            setConsensus({ live: "", content: null });
          } else if (type === "done") {
            const consensusText = (ev.consensus as string) ?? "";
            setConsensus({ live: consensusText, content: consensusText });
            setActiveAgent(null);
            setStatus("done");

            // Persist this deliberation to session history
            const completedTurns = completedTurnsRef.current.slice();
            const capturedQuery = currentQueryRef.current;
            const entry: CouncilHistoryEntry = {
              at: new Date().toISOString(),
              query: capturedQuery,
              turns: completedTurns,
              consensus: consensusText,
            };
            setSessionHistory((prev) => {
              const next = [...prev, entry];
              persistHistory(companyId, next);
              return next;
            });

            // Fire-and-forget to supermemory so this deliberation is searchable
            if (companyId && apiBase) {
              const memoryBody = JSON.stringify({
                title: `Council · "${capturedQuery.slice(0, 80)}${capturedQuery.length > 80 ? "…" : ""}"`,
                body: [
                  `Query: ${capturedQuery}`,
                  "",
                  "Deliberation:",
                  completedTurns
                    .map((t) => `[${t.agent} r${t.round + 1}]: ${t.content}`)
                    .join("\n\n"),
                  "",
                  "Resolution:",
                  consensusText,
                ].join("\n"),
                scope: "shared",
                source: "council_deliberation",
                kind: "general",
                tags: ["council", "deliberation"],
              });
              fetch(`${apiBase}/api/company/companies/${companyId}/memory`, {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: memoryBody,
              }).catch(() => {});
            }
          } else if (type === "error") {
            throw new Error((ev.message as string) ?? "Council error");
          }
        }
      }
    } catch (e) {
      if ((e as Error).name === "AbortError") return;
      setError(e instanceof Error ? e.message : String(e));
      setStatus("error");
    } finally {
      setActiveAgent(null);
    }
  }, [query, activeAgents, rounds, reset, sessionHistory, companyId, apiBase]);

  const isEmpty = turns.length === 0 && !consensus;
  const hasHistory = sessionHistory.length > 0;
  const showCurrentSection = !isEmpty || status === "error";

  return (
    <div className="flex h-full flex-col gap-0">
      {/* Header */}
      <div className="shrink-0 border-b border-[#1a1a1a] px-4 py-3">
        <div className="flex items-center gap-2">
          <Users className="size-4 text-[#666666]" strokeWidth={1.5} />
          <h1 className="font-mono text-[12px] font-medium uppercase tracking-widest text-[#c8c8c8]">
            Council chamber
          </h1>
          {status === "running" ? (
            <span className="ml-auto font-mono text-[9px] uppercase tracking-widest text-[#555555] animate-pulse">
              deliberating…
            </span>
          ) : hasHistory ? (
            <div className="ml-auto flex items-center gap-2">
              {historyCompacted ? (
                <span className="font-mono text-[9px] text-[#3a5a3a]" title="Older deliberations were compacted to fit the context window">
                  context compacted
                </span>
              ) : null}
              <div className="flex items-center gap-1 rounded border border-[#1a1a1a] bg-[#080808] px-2 py-0.5">
                <History className="size-2.5 text-[#3a3a3a]" strokeWidth={1.5} />
                <span className="font-mono text-[9px] text-[#3a3a3a]">
                  {sessionHistory.length} prior
                </span>
              </div>
              <button
                type="button"
                onClick={clearHistory}
                title="Clear council history"
                className="text-[#2a2a2a] hover:text-[#888888] transition-colors"
              >
                <X className="size-3" strokeWidth={1.5} />
              </button>
            </div>
          ) : null}
        </div>
        <p className="mt-1 text-[11px] text-[#555555]">
          Submit a task or question. Your agents deliberate in Socratic mode and reach a consensus on who should own it.
        </p>
      </div>

      {/* Agent roster */}
      <div className="shrink-0 border-b border-[#1a1a1a] px-4 py-2">
        {agentsLoading ? (
          <p className="font-mono text-[10px] uppercase tracking-wide text-[#444444]">Loading roster…</p>
        ) : activeAgents.length === 0 ? (
          <p className="font-mono text-[10px] uppercase tracking-wide text-[#444444]">
            No agents — add agents first in the Agents section.
          </p>
        ) : (
          <div className="flex flex-wrap gap-1.5">
            {activeAgents.map((a) => (
              <AgentChip
                key={a.id}
                agent={a}
                colourClass={agentColour(a.name, rosterNames)}
                active={status === "idle" || status === "done" || activeAgent === a.name}
              />
            ))}
          </div>
        )}
      </div>

      {/* Conversation feed */}
      <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto px-4 py-4">
        {!hasHistory && isEmpty && status === "idle" ? (
          <div className="flex h-full items-center justify-center">
            <p className="text-center text-sm text-[#444444]">
              No deliberation yet.
              <br />
              <span className="text-[11px]">Submit a task below to convene the council.</span>
            </p>
          </div>
        ) : (
          <div className="space-y-4">
            {/* Past deliberations — collapsed cards */}
            {hasHistory ? (
              <div className="space-y-1">
                {sessionHistory.map((entry, i) => (
                  <HistoryCard
                    key={entry.at}
                    entry={entry}
                    index={i}
                    rosterNames={rosterNames}
                  />
                ))}
              </div>
            ) : null}

            {/* Divider between history and live deliberation */}
            {hasHistory && showCurrentSection ? (
              <div className="flex items-center gap-2 py-1">
                <div className="flex-1 border-t border-[#0f0f0f]" />
                <span className="font-mono text-[9px] uppercase tracking-wide text-[#222222]">
                  current deliberation
                </span>
                <div className="flex-1 border-t border-[#0f0f0f]" />
              </div>
            ) : null}

            {/* Current deliberation turns */}
            {showCurrentSection ? (
              <div className="space-y-5">
                {turns.map((t, i) => (
                  <TurnBubble
                    key={`${t.agent}-${t.round}-${i}`}
                    turn={t}
                    agentColourClass={agentColour(t.agent, rosterNames)}
                    isLive={t.content === null}
                  />
                ))}
                {consensus ? <ConsensusBox consensus={consensus} /> : null}
                {status === "error" && error ? (
                  <p className="font-mono text-[10px] uppercase tracking-wide text-[#D4A843]">
                    [Error: {error}]
                  </p>
                ) : null}
              </div>
            ) : null}
          </div>
        )}
      </div>

      {/* Input bar */}
      <div className="shrink-0 border-t border-[#1a1a1a] bg-[#060606] p-3">
        <div className="flex gap-2">
          <Textarea
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                if (status !== "running") void start();
              }
            }}
            placeholder="What task or question should the council deliberate on? (Enter to submit)"
            rows={2}
            disabled={status === "running"}
            className="min-h-[44px] flex-1 resize-none border-[#2a2a2a] bg-black font-sans text-sm text-[#e8e8e8] placeholder:text-[#444444] focus-visible:border-[#444444] focus-visible:ring-1 focus-visible:ring-[#333333]"
          />
          <div className="flex flex-col gap-1.5">
            {status === "running" ? (
              <Button
                type="button"
                variant="outline"
                size="icon"
                title="Abort"
                className="size-11 shrink-0 border-[#333333] bg-black text-[#888888] hover:bg-white/[0.06]"
                onClick={reset}
              >
                <RotateCcw className="size-4" strokeWidth={1.5} />
              </Button>
            ) : (
              <Button
                type="button"
                variant="outline"
                size="icon"
                title="Convene council"
                disabled={!query.trim() || activeAgents.length === 0}
                className="size-11 shrink-0 border-[#2a2a2a] bg-black text-[#e8e8e8] hover:bg-white/[0.08] disabled:opacity-30"
                onClick={() => void start()}
              >
                <Send className="size-4" strokeWidth={1.5} />
              </Button>
            )}
            {/* Rounds selector */}
            <div className="flex flex-col items-center gap-0.5">
              <button
                type="button"
                onClick={() => setRounds((r) => Math.min(3, r + 1))}
                disabled={status === "running"}
                className="font-mono text-[9px] text-[#444444] hover:text-[#888888] disabled:opacity-30"
              >
                ▲
              </button>
              <span className="font-mono text-[9px] text-[#555555]">{rounds}r</span>
              <button
                type="button"
                onClick={() => setRounds((r) => Math.max(1, r - 1))}
                disabled={status === "running"}
                className="font-mono text-[9px] text-[#444444] hover:text-[#888888] disabled:opacity-30"
              >
                ▼
              </button>
            </div>
          </div>
        </div>
        {status === "done" ? (
          <button
            type="button"
            onClick={reset}
            className="mt-2 font-mono text-[10px] uppercase tracking-wide text-[#555555] hover:text-[#888888]"
          >
            ↺ New deliberation
          </button>
        ) : null}
      </div>
    </div>
  );
}
