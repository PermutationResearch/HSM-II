"use client";

import { useEffect, useMemo, useState } from "react";
import { AlertCircle, CheckCircle2, ChevronDown, FileText, Gauge, Info, Table2, Users } from "lucide-react";

import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/app/components/ui/collapsible";
import {
  buildYcBenchVisionCorpus,
  fetchCompanyYcBenchProfile,
  ycBenchStrategyControllerDisplay,
  type YcBenchProfileVisionFields,
} from "@/app/lib/fetch-company-yc-bench-profile";
import { fetchCompanyWorkspaceFile } from "@/app/lib/fetch-company-workspace-file";
import { lintPlaybookAgainstVision, type VisionLintMessage } from "@/app/lib/sop-vision-alignment";
import type { SopExampleDocument } from "@/app/lib/sop-examples-types";
import { cn } from "@/app/lib/utils";

const VISIONS_REL = "visions.md";

type Props = {
  apiBase: string;
  companyId: string;
  /** Company PATCH `context_markdown` — merged into vision corpus when file missing or thin. */
  contextMarkdown?: string | null;
  hsmiiHome?: string | null;
  sopDraft: SopExampleDocument;
};

function LintIcon({ level }: { level: VisionLintMessage["level"] }) {
  if (level === "ok") return <CheckCircle2 className="size-3.5 shrink-0 text-emerald-400/90" aria-hidden />;
  if (level === "warn") return <AlertCircle className="size-3.5 shrink-0 text-amber-400/90" aria-hidden />;
  return <Info className="size-3.5 shrink-0 text-sky-400/80" aria-hidden />;
}

export function SopVisionAlignment({ apiBase, companyId, contextMarkdown, hsmiiHome, sopDraft }: Props) {
  const [visionsFile, setVisionsFile] = useState<string | null>(null);
  const [visionsErr, setVisionsErr] = useState<string | null>(null);
  const [visionsLoading, setVisionsLoading] = useState(true);
  const [ycProfile, setYcProfile] = useState<YcBenchProfileVisionFields | null>(null);
  const [ycErr, setYcErr] = useState<string | null>(null);
  const [ycLoading, setYcLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setVisionsLoading(true);
    setYcLoading(true);
    setVisionsErr(null);
    setYcErr(null);
    void (async () => {
      const [fileRes, ycRes] = await Promise.all([
        fetchCompanyWorkspaceFile(apiBase, companyId, VISIONS_REL),
        fetchCompanyYcBenchProfile(apiBase, companyId),
      ]);
      if (cancelled) return;
      if (fileRes.ok) {
        setVisionsFile(fileRes.content);
        setVisionsErr(null);
      } else {
        setVisionsFile(null);
        setVisionsErr(fileRes.error);
      }
      setVisionsLoading(false);
      if (ycRes.ok) {
        setYcProfile(ycRes.profile);
        setYcErr(null);
      } else {
        setYcProfile(null);
        setYcErr(ycRes.error);
      }
      setYcLoading(false);
    })();
    return () => {
      cancelled = true;
    };
  }, [apiBase, companyId]);

  const ctx = (contextMarkdown ?? "").trim();
  const fileTrim = (visionsFile ?? "").trim();

  const ycVisionCorpus = useMemo(
    () => (ycProfile ? buildYcBenchVisionCorpus(ycProfile) : ""),
    [ycProfile],
  );
  const ycTrim = ycVisionCorpus.trim();
  const strategyControllerBlock = useMemo(
    () => (ycProfile ? ycBenchStrategyControllerDisplay(ycProfile).trim() : ""),
    [ycProfile],
  );

  const visionCorpus = useMemo(() => {
    const parts: string[] = [];
    if (fileTrim) parts.push("--- visions.md ---\n", fileTrim);
    if (ycTrim) parts.push("\n--- YC-Bench profile (API) ---\n", ycVisionCorpus);
    if (ctx) parts.push("\n--- Shared context (API) ---\n", ctx);
    return parts.join("\n").trim();
  }, [fileTrim, ycTrim, ycVisionCorpus, ctx]);

  const hadVisionsFile = fileTrim.length > 0;
  const hadYcBenchProfile = ycTrim.length > 0;
  const hadContextMarkdown = ctx.length > 0;

  const lint = useMemo(
    () =>
      lintPlaybookAgainstVision(visionCorpus, sopDraft, {
        hadVisionsFile,
        hadYcBenchProfile,
        hadContextMarkdown,
      }),
    [visionCorpus, sopDraft, hadVisionsFile, hadYcBenchProfile, hadContextMarkdown],
  );

  /** Omit the visions.md row when YC-Bench already fills the vision corpus — nothing useful to show. */
  const showVisionsFileSection = fileTrim || visionsLoading || !ycTrim;

  const previewText = useMemo(() => {
    if (visionsLoading) return "Loading visions.md…";
    if (fileTrim) return fileTrim;
    if (visionsErr && visionsErr.toLowerCase().includes("no hsmii_home")) {
      return "Set hsmii_home on the company to load visions.md from disk.";
    }
    if (visionsErr) {
      const el = visionsErr.toLowerCase();
      if (el.includes("not a file")) {
        return [
          "There is no regular file at visions.md under hsmii_home — usually it has not been created yet.",
          "Create a file named visions.md at your company pack root (same folder as agents/), or delete/rename if visions.md is a folder.",
          "If the YC-Bench profile (below) or Shared context is set, alignment can still run without this file.",
        ].join(" ");
      }
      return `Could not read visions.md (${visionsErr}).`;
    }
    return "No visions.md on disk yet — add it at the pack root, or rely on the YC-Bench profile and Shared context below.";
  }, [visionsLoading, fileTrim, visionsErr]);

  return (
    <div className="space-y-3 rounded-lg border border-violet-500/25 bg-violet-950/20 p-4 ring-1 ring-violet-500/10">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div>
          <p className="font-mono text-[10px] font-semibold uppercase tracking-[0.12em] text-violet-300/90">
            Vision alignment
          </p>
          <p className="mt-1 max-w-3xl text-xs leading-relaxed text-gray-400">
            Same vision-relevant inputs as the marketplace <span className="font-mono text-[11px]">YC-Bench profile</span>:
            strategy, controller, <span className="font-mono text-[11px]">RAT</span> domains, company signals, imported
            skills, agent hints, and (for lint) workforce wording. Plus company{" "}
            <span className="font-mono text-[11px]">Shared context</span> (API). If present,{" "}
            <span className="font-mono text-[11px]">{VISIONS_REL}</span> under{" "}
            <span className="font-mono text-[11px] text-gray-300">hsmii_home</span>
            {hsmiiHome ? (
              <>
                {" "}
                (<span className="truncate font-mono text-[10px] text-muted-foreground" title={hsmiiHome}>
                  {hsmiiHome}
                </span>
                )
              </>
            ) : null}{" "}
            is merged in too. Lint is lexical overlap — not semantic review.
          </p>
        </div>
        {lint.coverage != null ? (
          <div className="rounded border border-line bg-black/30 px-2 py-1 font-mono text-[10px] text-gray-400">
            token overlap{" "}
            <span className="text-gray-200">
              {lint.matchedTokenCount}/{lint.visionTokenCount}
            </span>{" "}
            ({(lint.coverage * 100).toFixed(0)}%)
          </div>
        ) : null}
      </div>

      {showVisionsFileSection ? (
        <Collapsible defaultOpen={!fileTrim && !ctx && !ycTrim}>
          <CollapsibleTrigger className="flex w-full items-center gap-2 rounded-md border border-line bg-black/30 px-3 py-2 text-left text-xs font-medium text-gray-300 hover:bg-black/45 [&[data-state=open]>svg]:rotate-180">
            <ChevronDown className="size-4 shrink-0 transition-transform" aria-hidden />
            <FileText className="size-4 shrink-0 text-violet-400/80" aria-hidden />
            <span>
              visions.md {fileTrim ? "— on disk" : visionsLoading ? "— loading…" : "— not on disk"}
            </span>
          </CollapsibleTrigger>
          <CollapsibleContent className="mt-2 max-h-[min(40vh,280px)] overflow-auto rounded-md border border-line/80 bg-black/35 p-3 font-mono text-[11px] leading-relaxed text-gray-300 whitespace-pre-wrap">
            {visionsLoading ? "Loading visions.md…" : previewText}
          </CollapsibleContent>
        </Collapsible>
      ) : null}

      <Collapsible defaultOpen={!fileTrim && Boolean(strategyControllerBlock || ycTrim)}>
        <CollapsibleTrigger className="flex w-full items-center gap-2 rounded-md border border-line bg-black/30 px-3 py-2 text-left text-xs font-medium text-gray-300 hover:bg-black/45 [&[data-state=open]>svg]:rotate-180">
          <ChevronDown className="size-4 shrink-0 transition-transform" aria-hidden />
          <Gauge className="size-4 shrink-0 text-fuchsia-400/85" aria-hidden />
          <span>
            YC-Bench — strategy & controller
            {strategyControllerBlock || ycTrim
              ? " — merged into lint"
              : ycLoading
                ? " — loading"
                : ycErr
                  ? " — error"
                  : " — empty"}
          </span>
        </CollapsibleTrigger>
        <CollapsibleContent className="mt-2 max-h-[min(36vh,260px)] overflow-auto rounded-md border border-line/80 bg-black/35 p-3 font-mono text-[10px] leading-relaxed text-gray-300 whitespace-pre-wrap">
          {ycLoading ? (
            "Loading YC-Bench profile…"
          ) : ycErr ? (
            <span className="text-amber-200/90">Could not load profile ({ycErr}). Vision alignment will use visions.md and Shared context only.</span>
          ) : strategyControllerBlock ? (
            strategyControllerBlock
          ) : ycTrim ? (
            "Strategy and controller blocks are empty — see the RAT (domain scores) section below."
          ) : (
            "No YC-Bench profile text yet — add company context, agents, and skills so the benchmark controller can summarize strategy."
          )}
        </CollapsibleContent>
      </Collapsible>

      {!ycLoading && !ycErr && ycProfile ? (
        <Collapsible defaultOpen={!fileTrim && (ycProfile.domain_scores.length > 0 || ycProfile.top_domains.length > 0)}>
          <CollapsibleTrigger className="flex w-full items-center gap-2 rounded-md border border-line bg-black/30 px-3 py-2 text-left text-xs font-medium text-gray-300 hover:bg-black/45 [&[data-state=open]>svg]:rotate-180">
            <ChevronDown className="size-4 shrink-0 transition-transform" aria-hidden />
            <Table2 className="size-4 shrink-0 text-cyan-400/85" aria-hidden />
            <span>
              YC-Bench RAT (risk alignment table)
              {ycProfile.domain_scores.length > 0 || ycProfile.top_domains.length > 0
                ? " — merged into lint"
                : " — no rows yet"}
            </span>
          </CollapsibleTrigger>
          <CollapsibleContent className="mt-2 overflow-auto rounded-md border border-line/80 bg-black/35 p-3">
            {ycProfile.top_domains.length > 0 ? (
              <p className="mb-3 font-mono text-[10px] leading-relaxed text-gray-400">
                <span className="text-gray-500">Top domains: </span>
                {ycProfile.top_domains.join(", ")}
              </p>
            ) : null}
            {ycProfile.domain_scores.length > 0 ? (
              <table className="w-full border-collapse text-left font-mono text-[10px] text-gray-300">
                <thead>
                  <tr className="border-b border-line/80 text-gray-500">
                    <th className="py-1.5 pr-2 font-medium">Domain</th>
                    <th className="py-1.5 pr-2 font-medium">Score</th>
                    <th className="py-1.5 pr-2 font-medium">Matched terms</th>
                    <th className="py-1.5 font-medium">Evidence</th>
                  </tr>
                </thead>
                <tbody>
                  {ycProfile.domain_scores
                    .slice()
                    .sort((a, b) => b.score - a.score)
                    .map((row) => (
                      <tr key={row.domain} className="border-b border-line/40 align-top">
                        <td className="py-2 pr-2 text-gray-200">{row.domain}</td>
                        <td className="py-2 pr-2 whitespace-nowrap">{row.score.toFixed(2)}</td>
                        <td className="py-2 pr-2 text-gray-400">{row.matched_terms.join(", ") || "—"}</td>
                        <td className="py-2 text-gray-500">
                          {row.evidence.length > 0 ? (
                            <ul className="list-inside list-disc space-y-1">
                              {row.evidence.slice(0, 4).map((e, i) => (
                                <li key={i} className="break-words">
                                  {e}
                                </li>
                              ))}
                            </ul>
                          ) : (
                            "—"
                          )}
                        </td>
                      </tr>
                    ))}
                </tbody>
              </table>
            ) : (
              <p className="font-mono text-[10px] leading-relaxed text-gray-500">
                No domain-score rows yet — they fill in when company context, agents, or skills mention the strategic
                domains (research, inference, training, data environment).
              </p>
            )}
            <p className="mt-3 border-t border-line/50 pt-2 text-[10px] leading-relaxed text-gray-500">
              RAT is the same <span className="font-mono text-gray-400">domain_scores</span> table used for YC-Bench
              marketplace profiling — not a separate artifact.
            </p>
          </CollapsibleContent>
        </Collapsible>
      ) : null}

      {!ycLoading && !ycErr && ycProfile ? (
        <Collapsible defaultOpen={!fileTrim && ycProfile.agent_hints.length > 0}>
          <CollapsibleTrigger className="flex w-full items-center gap-2 rounded-md border border-line bg-black/30 px-3 py-2 text-left text-xs font-medium text-gray-300 hover:bg-black/45 [&[data-state=open]>svg]:rotate-180">
            <ChevronDown className="size-4 shrink-0 transition-transform" aria-hidden />
            <Users className="size-4 shrink-0 text-amber-400/85" aria-hidden />
            <span>
              YC-Bench — company signals & agents
              {ycProfile.source.agent_count > 0 || ycProfile.agent_hints.length > 0 ? " — merged into lint" : ""}
            </span>
          </CollapsibleTrigger>
          <CollapsibleContent className="mt-2 space-y-3 rounded-md border border-line/80 bg-black/35 p-3">
            <div className="rounded-md border border-line/60 bg-black/25 p-3">
              <div className="mb-2 text-[10px] font-medium uppercase tracking-wide text-gray-500">Company signals</div>
              <div className="space-y-1.5 text-sm text-gray-400">
                <p>
                  {ycProfile.source.agent_count} imported agent{ycProfile.source.agent_count === 1 ? "" : "s"}
                </p>
                <p>
                  {ycProfile.source.skill_count} imported skill template{ycProfile.source.skill_count === 1 ? "" : "s"}
                </p>
                <p>{ycProfile.source.has_context_markdown ? "Shared context (API) present" : "No Shared context yet"}</p>
              </div>
            </div>
            <div className="rounded-md border border-line/60 bg-black/25 p-3">
              <div className="mb-2 text-[10px] font-medium uppercase tracking-wide text-gray-500">Imported skill templates</div>
              {ycProfile.imported_skills.length === 0 ? (
                <p className="text-sm text-gray-500">None imported yet.</p>
              ) : (
                <p className="font-mono text-[10px] leading-relaxed text-gray-300">{ycProfile.imported_skills.join(", ")}</p>
              )}
            </div>
            <div className="rounded-md border border-line/60 bg-black/25 p-3">
              <div className="mb-2 text-[10px] font-medium uppercase tracking-wide text-gray-500">Agent hints</div>
              <ul className="space-y-2 text-sm text-gray-400">
                {ycProfile.agent_hints.length === 0 ? (
                  <li>No imported agents yet.</li>
                ) : (
                  ycProfile.agent_hints.slice(0, 12).map((agent) => (
                    <li key={agent.id || agent.display_name}>
                      <span className="text-gray-200">{agent.display_name}</span> · {agent.role}
                      {agent.matched_domains.length > 0 ? ` · ${agent.matched_domains.join(", ")}` : ""}
                    </li>
                  ))
                )}
              </ul>
            </div>
          </CollapsibleContent>
        </Collapsible>
      ) : null}

      {ctx ? (
        <Collapsible defaultOpen={!fileTrim}>
          <CollapsibleTrigger className="flex w-full items-center gap-2 rounded-md border border-line bg-black/25 px-3 py-2 text-left text-xs font-medium text-gray-400 hover:bg-black/35 [&[data-state=open]>svg]:rotate-180">
            <ChevronDown className="size-4 shrink-0 transition-transform" aria-hidden />
            Shared context (API){fileTrim ? " — merged into lint" : " — used in lint"}
          </CollapsibleTrigger>
          <CollapsibleContent className="mt-2 max-h-[min(28vh,200px)] overflow-auto rounded-md border border-line/60 bg-black/25 p-3 font-mono text-[10px] text-gray-400 whitespace-pre-wrap">
            {ctx}
          </CollapsibleContent>
        </Collapsible>
      ) : null}

      <div className="space-y-2" role="status" aria-live="polite">
        {lint.messages.map((m, i) => (
          <div
            key={i}
            className={cn(
              "flex gap-2 rounded-md border px-2.5 py-2 text-xs leading-snug",
              m.level === "warn" && "border-amber-500/35 bg-amber-950/25 text-amber-100/95",
              m.level === "ok" && "border-emerald-500/30 bg-emerald-950/20 text-emerald-100/90",
              m.level === "info" && "border-sky-500/25 bg-sky-950/20 text-sky-100/85",
            )}
          >
            <LintIcon level={m.level} />
            <span>{m.text}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
