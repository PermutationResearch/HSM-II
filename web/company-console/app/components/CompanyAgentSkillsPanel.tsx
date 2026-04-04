"use client";

import { useEffect, useMemo, useState } from "react";

import type { CoAgentRow } from "./CompanyAgentsPanel";

type CompanySkillTemplate = {
  id: string;
  slug: string;
  name: string;
  description: string;
  body: string;
  skill_path: string;
  source: string;
  updated_at: string;
};

type Props = {
  api: string;
  companyId: string;
  agents: CoAgentRow[];
  setCoErr: (msg: string | null) => void;
  onOpenSkill?: (slug: string) => void;
};

type AgentSkillBinding = {
  agent: CoAgentRow;
  source: "paperclip" | "capabilities" | "none";
  matched: CompanySkillTemplate[];
  missing: string[];
};

function normalizeSkillRef(raw: string): string {
  const value = raw.trim();
  if (!value) return "";
  const pathMatch = value.match(/skills\/([^/]+)\/SKILL\.md$/i);
  if (pathMatch) return pathMatch[1].trim();
  return value.replace(/^skills\//i, "").replace(/\/SKILL\.md$/i, "").replace(/\/+$/g, "").trim();
}

function parsePaperclipSkills(cfg: unknown): string[] {
  if (!cfg || typeof cfg !== "object" || Array.isArray(cfg)) return [];
  const paperclip = (cfg as Record<string, unknown>).paperclip;
  if (!paperclip || typeof paperclip !== "object" || Array.isArray(paperclip)) return [];
  const skills = (paperclip as Record<string, unknown>).skills;
  if (!Array.isArray(skills)) return [];
  return [...new Set(skills.filter((value): value is string => typeof value === "string").map(normalizeSkillRef).filter(Boolean))];
}

function parseCapabilitiesSkills(capabilities: string | null | undefined): string[] {
  if (!capabilities) return [];
  return [
    ...new Set(
      capabilities
        .split(/[\n,;|]/)
        .map(normalizeSkillRef)
        .filter(Boolean)
    ),
  ];
}

export function CompanyAgentSkillsPanel({ api, companyId, agents, setCoErr, onOpenSkill }: Props) {
  const [skills, setSkills] = useState<CompanySkillTemplate[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    let cancelled = false;

    const load = async () => {
      setLoading(true);
      try {
        const r = await fetch(`${api}/api/company/companies/${companyId}/skills`);
        const j = (await r.json()) as { skills?: CompanySkillTemplate[]; error?: string };
        if (!r.ok) {
          throw new Error(j.error ?? r.statusText);
        }
        if (!cancelled) {
          setSkills(Array.isArray(j.skills) ? j.skills : []);
        }
      } catch (e) {
        if (!cancelled) {
          setSkills([]);
          setCoErr(e instanceof Error ? e.message : String(e));
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    };

    void load();
    return () => {
      cancelled = true;
    };
  }, [api, companyId, setCoErr]);

  const skillBySlug = useMemo(
    () => new Map(skills.map((skill) => [normalizeSkillRef(skill.slug), skill])),
    [skills]
  );
  const agentById = useMemo(() => new Map(agents.map((agent) => [agent.id, agent])), [agents]);

  const bindings = useMemo<AgentSkillBinding[]>(() => {
    return agents.map((agent) => {
      const importedRefs = parsePaperclipSkills(agent.adapter_config);
      const fallbackRefs = importedRefs.length === 0 ? parseCapabilitiesSkills(agent.capabilities) : [];
      const refs = importedRefs.length > 0 ? importedRefs : fallbackRefs;
      const matched: CompanySkillTemplate[] = [];
      const missing: string[] = [];

      for (const ref of refs) {
        const match = skillBySlug.get(ref);
        if (match) matched.push(match);
        else missing.push(ref);
      }

      return {
        agent,
        source: importedRefs.length > 0 ? "paperclip" : fallbackRefs.length > 0 ? "capabilities" : "none",
        matched,
        missing,
      };
    });
  }, [agents, skillBySlug]);

  const linkedAgents = useMemo(
    () => bindings.filter((binding) => binding.matched.length > 0 || binding.missing.length > 0).length,
    [bindings]
  );

  return (
    <details className="mb-6 rounded-lg border border-line bg-panel" open>
      <summary className="cursor-pointer list-none px-4 py-3 text-sm font-medium text-gray-200 marker:content-none [&::-webkit-details-marker]:hidden">
        <span className="text-gray-400">▸</span> Agent skill map{" "}
        <span className="font-normal text-gray-500">
          (joined view of workforce agents and imported <code className="text-xs text-accent">skills/</code>)
        </span>
      </summary>
      <div className="space-y-4 border-t border-line px-4 py-4">
        <p className="text-sm leading-relaxed text-gray-500">
          This is the missing joined table: each agent shows the skill refs imported from the pack. HSM-II prefers the
          explicit <code className="text-xs text-accent">adapter_config.paperclip.skills</code> list and only falls
          back to comma-separated <strong className="text-gray-400">capabilities</strong> when older rows do not have
          the import metadata yet.
        </p>

        <div className="flex flex-wrap items-center gap-2 text-xs text-gray-600">
          <span>{loading ? "Refreshing skill links…" : `${linkedAgents}/${agents.length} agent${agents.length === 1 ? "" : "s"} linked`}</span>
          <span>{skills.length} imported template{skills.length === 1 ? "" : "s"} available</span>
        </div>

        {bindings.length === 0 ? (
          <div className="rounded-lg border border-dashed border-line/80 bg-black/20 px-4 py-3 text-sm text-gray-500">
            No workforce agents yet. Import a pack or add agents below to populate the co-working map.
          </div>
        ) : (
          <ul className="space-y-3">
            {bindings.map(({ agent, source, matched, missing }) => (
              <li key={agent.id} className="rounded-lg border border-line bg-ink/30 px-3 py-3">
                <div className="flex flex-wrap items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-medium text-white">{agent.title?.trim() || agent.name}</span>
                      <span className="rounded border border-line/80 px-1.5 py-px font-mono text-[10px] uppercase tracking-wide text-accent">
                        {agent.name}
                      </span>
                      <span className="text-xs text-gray-500">{agent.role}</span>
                    </div>
                    {agent.title?.trim() ? <p className="mt-1 text-xs text-gray-500">{agent.name}</p> : null}
                    <p className="mt-1 text-xs text-gray-500">
                      Reports to {agent.reports_to ? agentById.get(agent.reports_to)?.title?.trim() || agentById.get(agent.reports_to)?.name || agent.reports_to : "top level"}
                    </p>
                  </div>
                  <span className="rounded border border-line/80 px-2 py-1 text-[11px] text-gray-500">
                    {source === "paperclip"
                      ? "from imported pack config"
                      : source === "capabilities"
                        ? "from capabilities fallback"
                        : "no linked skills"}
                  </span>
                </div>

                {matched.length === 0 && missing.length === 0 ? (
                  <p className="mt-3 text-sm text-gray-600">No skill refs found on this agent yet.</p>
                ) : (
                  <div className="mt-3 flex flex-wrap gap-2">
                    {matched.map((skill) => (
                      <button
                        type="button"
                        key={`${agent.id}-${skill.id}`}
                        className="rounded-full border border-emerald-700/50 bg-emerald-500/10 px-2.5 py-1 text-xs text-emerald-200 transition hover:border-emerald-500 hover:bg-emerald-500/20"
                        title={skill.description || skill.skill_path}
                        onClick={() => onOpenSkill?.(normalizeSkillRef(skill.slug))}
                      >
                        {skill.name || skill.slug}
                      </button>
                    ))}
                    {missing.map((slug) => (
                      <span
                        key={`${agent.id}-${slug}`}
                        className="rounded-full border border-amber-700/50 bg-amber-500/10 px-2.5 py-1 text-xs text-amber-200"
                        title="Referenced by the agent, but not found in imported skill templates."
                      >
                        {slug} missing template
                      </span>
                    ))}
                  </div>
                )}

                {agent.briefing?.trim() ? (
                  <p className="mt-3 line-clamp-3 text-sm leading-relaxed text-gray-500">{agent.briefing.trim()}</p>
                ) : null}
              </li>
            ))}
          </ul>
        )}
      </div>
    </details>
  );
}
