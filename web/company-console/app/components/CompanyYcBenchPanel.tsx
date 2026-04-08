"use client";

import { ChevronRight } from "lucide-react";
import { useEffect, useState } from "react";

type YcBenchDomainScore = {
  domain: string;
  score: number;
  matched_terms: string[];
  evidence: string[];
};

type YcBenchAgentHint = {
  id: string;
  display_name: string;
  role: string;
  matched_domains: string[];
};

type CompanyYcBenchProfile = {
  company_id: string;
  slug: string;
  display_name: string;
  issue_key_prefix: string;
  generated_at: string;
  source: {
    agent_count: number;
    skill_count: number;
    has_context_markdown: boolean;
  };
  top_domains: string[];
  domain_scores: YcBenchDomainScore[];
  agent_hints: YcBenchAgentHint[];
  imported_skills: string[];
  strategy_summary: string;
  controller_prompt: string;
  benchmark_spec: {
    labels: Record<string, unknown>;
    setup_commands: string[][];
    command: string[];
    cwd_hint: string;
    notes: string[];
  };
};

type Props = {
  api: string;
  companyId: string;
  setCoErr: (msg: string | null) => void;
};

export function CompanyYcBenchPanel({ api, companyId, setCoErr }: Props) {
  const [profile, setProfile] = useState<CompanyYcBenchProfile | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    let cancelled = false;

    const load = async () => {
      setLoading(true);
      try {
        const r = await fetch(`${api}/api/company/companies/${companyId}/yc-bench-profile`);
        const j = (await r.json()) as { profile?: CompanyYcBenchProfile; error?: string };
        if (!r.ok) {
          throw new Error(j.error ?? r.statusText);
        }
        if (!cancelled) {
          setProfile(j.profile ?? null);
        }
      } catch (e) {
        if (!cancelled) {
          setProfile(null);
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

  return (
    <details className="group mb-6 rounded-xl border border-[#30363D] bg-[#0d1117]">
      <summary className="flex cursor-pointer list-none items-start gap-2 px-4 py-3.5 marker:content-none [&::-webkit-details-marker]:hidden">
        <ChevronRight
          className="mt-0.5 h-4 w-4 shrink-0 text-[#8B949E] transition-transform duration-200 group-open:rotate-90"
          aria-hidden
        />
        <div className="min-w-0 flex-1">
          <span className="text-sm font-medium text-white">YC-Bench profile</span>
          <p className="mt-1 text-xs leading-relaxed text-[#8B949E]">
            Auto-built summary for marketplace benchmarks — optional unless you run YC-Bench workflows.
          </p>
        </div>
      </summary>
      <div className="space-y-4 border-t border-[#30363D] px-4 py-4">
        {loading || !profile ? (
          <div className="rounded-lg border border-dashed border-line/80 bg-black/20 px-4 py-3 text-sm text-gray-500">
            {loading ? "Building YC-Bench controller profile…" : "No YC-Bench profile available yet."}
          </div>
        ) : (
          <>
            <p className="text-sm leading-relaxed text-gray-500">{profile.strategy_summary}</p>

            <div className="flex flex-wrap gap-2 text-xs">
              {profile.domain_scores.slice(0, 4).map((domain) => (
                <span
                  key={domain.domain}
                  className="rounded-full border border-[#58a6ff]/35 bg-[#58a6ff]/10 px-2.5 py-1 text-[#9ecbff]"
                  title={domain.matched_terms.join(", ")}
                >
                  {domain.domain} {domain.score.toFixed(1)}
                </span>
              ))}
            </div>

            <div className="grid gap-4 lg:grid-cols-[1.2fr,0.8fr]">
              <div className="rounded-lg border border-line bg-black/20 p-3">
                <div className="mb-2 text-xs font-medium uppercase tracking-wide text-gray-500">Controller prompt</div>
                <pre className="max-h-[320px] overflow-auto rounded-lg border border-line bg-black/30 px-3 py-3 font-mono text-xs leading-relaxed text-gray-300">
                  <code>{profile.controller_prompt}</code>
                </pre>
              </div>

              <div className="space-y-4">
                <div className="rounded-lg border border-line bg-black/20 p-3">
                  <div className="mb-2 text-xs font-medium uppercase tracking-wide text-gray-500">Company signals</div>
                  <div className="space-y-2 text-sm text-gray-400">
                    <p>{profile.source.agent_count} imported agent{profile.source.agent_count === 1 ? "" : "s"}</p>
                    <p>{profile.source.skill_count} imported skill template{profile.source.skill_count === 1 ? "" : "s"}</p>
                    <p>{profile.source.has_context_markdown ? "Context markdown imported" : "No context markdown yet"}</p>
                  </div>
                </div>

                <div className="rounded-lg border border-line bg-black/20 p-3">
                  <div className="mb-2 text-xs font-medium uppercase tracking-wide text-gray-500">Agent hints</div>
                  <ul className="space-y-2 text-sm text-gray-400">
                    {profile.agent_hints.length === 0 ? (
                      <li>No imported agents yet.</li>
                    ) : (
                      profile.agent_hints.slice(0, 6).map((agent) => (
                        <li key={agent.id}>
                          <span className="text-white">{agent.display_name}</span> · {agent.role}
                          {agent.matched_domains.length > 0 ? ` · ${agent.matched_domains.join(", ")}` : ""}
                        </li>
                      ))
                    )}
                  </ul>
                </div>
              </div>
            </div>

            <div className="rounded-lg border border-line bg-black/20 p-3">
              <div className="mb-2 text-xs font-medium uppercase tracking-wide text-gray-500">Suggested YC-Bench run template</div>
              <pre className="max-h-[260px] overflow-auto rounded-lg border border-line bg-black/30 px-3 py-3 font-mono text-xs leading-relaxed text-gray-300">
                <code>{JSON.stringify(profile.benchmark_spec, null, 2)}</code>
              </pre>
            </div>
          </>
        )}
      </div>
    </details>
  );
}
