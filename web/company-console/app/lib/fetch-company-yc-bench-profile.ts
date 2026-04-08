import { companyOsUrl } from "@/app/lib/company-api-url";

/** One row of the strategic-domain alignment table (YC-Bench “RAT” — domain risk/alignment signals). */
export type YcBenchDomainScore = {
  domain: string;
  score: number;
  matched_terms: string[];
  evidence: string[];
};

/** Same shape as `YcBenchAgentHint` in the Company OS API. */
export type YcBenchAgentHint = {
  id: string;
  display_name: string;
  role: string;
  matched_domains: string[];
};

/** Same shape as `YcBenchProfileSource` in the Company OS API. */
export type YcBenchProfileSource = {
  agent_count: number;
  skill_count: number;
  has_context_markdown: boolean;
};

/** Full profile fields — matches `CompanyYcBenchProfile` from GET …/yc-bench-profile (marketplace panel). */
export type YcBenchProfileVisionFields = {
  strategy_summary: string;
  controller_prompt: string;
  top_domains: string[];
  domain_scores: YcBenchDomainScore[];
  source: YcBenchProfileSource;
  agent_hints: YcBenchAgentHint[];
  imported_skills: string[];
};

export type FetchYcBenchProfileResult =
  | { ok: true; profile: YcBenchProfileVisionFields }
  | { ok: false; status: number; error: string };

const MAX_VISION_CHARS = 14_000;

function parseDomainScore(raw: unknown): YcBenchDomainScore | null {
  if (!raw || typeof raw !== "object") return null;
  const o = raw as Record<string, unknown>;
  const domain = typeof o.domain === "string" ? o.domain : "";
  const scoreRaw = o.score;
  const score =
    typeof scoreRaw === "number"
      ? scoreRaw
      : typeof scoreRaw === "string"
        ? Number.parseFloat(scoreRaw)
        : Number(scoreRaw);
  const matched_terms = Array.isArray(o.matched_terms)
    ? o.matched_terms.filter((t): t is string => typeof t === "string")
    : [];
  const evidence = Array.isArray(o.evidence) ? o.evidence.filter((t): t is string => typeof t === "string") : [];
  if (!domain || Number.isNaN(score)) return null;
  return { domain, score, matched_terms, evidence };
}

function parseAgentHint(raw: unknown): YcBenchAgentHint | null {
  if (!raw || typeof raw !== "object") return null;
  const o = raw as Record<string, unknown>;
  const id = typeof o.id === "string" ? o.id : "";
  const display_name = typeof o.display_name === "string" ? o.display_name : "";
  const role = typeof o.role === "string" ? o.role : "";
  const matched_domains = Array.isArray(o.matched_domains)
    ? o.matched_domains.filter((t): t is string => typeof t === "string")
    : [];
  if (!display_name && !id) return null;
  return { id, display_name, role, matched_domains };
}

function parseSource(raw: unknown): YcBenchProfileSource {
  if (!raw || typeof raw !== "object") {
    return { agent_count: 0, skill_count: 0, has_context_markdown: false };
  }
  const o = raw as Record<string, unknown>;
  return {
    agent_count: typeof o.agent_count === "number" ? o.agent_count : Number(o.agent_count) || 0,
    skill_count: typeof o.skill_count === "number" ? o.skill_count : Number(o.skill_count) || 0,
    has_context_markdown: Boolean(o.has_context_markdown),
  };
}

/**
 * Loads the deterministic YC-Bench controller profile (strategy, controller prompt, domain RAT).
 * Built from company context, agents, skills — aligns with how YC-Bench scores each pack.
 */
export async function fetchCompanyYcBenchProfile(
  apiBase: string,
  companyId: string,
): Promise<FetchYcBenchProfileResult> {
  const url = companyOsUrl(apiBase, `/api/company/companies/${companyId}/yc-bench-profile`);
  const r = await fetch(url);
  const j = (await r.json().catch(() => ({}))) as {
    error?: string;
    profile?: {
      strategy_summary?: string;
      controller_prompt?: string;
      top_domains?: unknown;
      domain_scores?: unknown;
      source?: unknown;
      agent_hints?: unknown;
      imported_skills?: unknown;
    };
  };
  if (!r.ok) {
    return { ok: false, status: r.status, error: typeof j.error === "string" ? j.error : r.statusText };
  }
  const p = j.profile;
  if (!p) {
    return { ok: false, status: r.status, error: "response missing profile" };
  }
  const rawScores = Array.isArray(p.domain_scores) ? p.domain_scores : [];
  const domain_scores = rawScores.map(parseDomainScore).filter((x): x is YcBenchDomainScore => x != null);
  const top_domains = Array.isArray(p.top_domains)
    ? p.top_domains.filter((t): t is string => typeof t === "string")
    : [];
  const rawHints = Array.isArray(p.agent_hints) ? p.agent_hints : [];
  const agent_hints = rawHints.map(parseAgentHint).filter((x): x is YcBenchAgentHint => x != null);
  const imported_skills = Array.isArray(p.imported_skills)
    ? p.imported_skills.filter((t): t is string => typeof t === "string")
    : [];
  const source = parseSource(p.source);
  return {
    ok: true,
    profile: {
      strategy_summary: typeof p.strategy_summary === "string" ? p.strategy_summary : "",
      controller_prompt: typeof p.controller_prompt === "string" ? p.controller_prompt : "",
      top_domains,
      domain_scores,
      source,
      agent_hints,
      imported_skills,
    },
  };
}

/** Markdown-style block of domain scores for lexical lint (merged into vision corpus). */
export function ycBenchDomainScoresToVisionText(scores: YcBenchDomainScore[], topDomains: string[]): string {
  if (scores.length === 0 && topDomains.length === 0) return "";
  const lines: string[] = ["--- YC-Bench RAT (strategic domain scores) ---"];
  if (topDomains.length > 0) {
    lines.push(`Top domains: ${topDomains.join(", ")}`);
  }
  const sorted = scores.slice().sort((a, b) => b.score - a.score);
  for (const d of sorted) {
    const terms = d.matched_terms.join(", ");
    const ev0 = d.evidence[0]?.slice(0, 200) ?? "";
    lines.push(
      `${d.domain}: score ${d.score.toFixed(2)}${terms ? `; terms: ${terms}` : ""}${ev0 ? `; evidence: ${ev0}` : ""}`,
    );
  }
  return lines.join("\n");
}

/** Workforce / skill lines for lexical lint (parity with marketplace YC-Bench panel). */
export function ycBenchWorkforceSignalsToVisionText(p: YcBenchProfileVisionFields): string {
  const lines: string[] = ["--- YC-Bench workforce & skills ---"];
  const s = p.source;
  lines.push(
    `Imported agents: ${s.agent_count}, skill templates: ${s.skill_count}, company Shared context (API): ${s.has_context_markdown ? "present" : "empty"}`,
  );
  if (p.imported_skills.length > 0) {
    lines.push(`Imported skill templates: ${p.imported_skills.join(", ")}`);
  }
  for (const h of p.agent_hints) {
    const dom = h.matched_domains.length > 0 ? ` — domains: ${h.matched_domains.join(", ")}` : "";
    lines.push(`Workforce agent: ${h.display_name} (${h.role})${dom}`);
  }
  if (lines.length <= 1) return "";
  return lines.join("\n");
}

/**
 * Strategy + controller + RAT lines for lexical lint; cap size for the browser.
 * @deprecated Prefer {@link buildYcBenchVisionCorpus} which includes RAT.
 */
export function ycBenchProfileToVisionText(p: YcBenchProfileVisionFields): string {
  return buildYcBenchVisionCorpus(p);
}

/** Full YC-Bench text merged into playbook vision lint (strategy, controller, RAT, workforce — same inputs as marketplace panel). */
export function buildYcBenchVisionCorpus(p: YcBenchProfileVisionFields): string {
  const a = p.strategy_summary.trim();
  const b = p.controller_prompt.trim();
  const rat = ycBenchDomainScoresToVisionText(p.domain_scores, p.top_domains).trim();
  const workforce = ycBenchWorkforceSignalsToVisionText(p).trim();
  let out = [a, b, rat, workforce].filter(Boolean).join("\n\n");
  if (out.length > MAX_VISION_CHARS) {
    out = `${out.slice(0, MAX_VISION_CHARS)}\n\n… [truncated]`;
  }
  return out;
}

/** Plaintext strategy + controller for display (RAT is shown in its own panel). */
export function ycBenchStrategyControllerDisplay(p: YcBenchProfileVisionFields): string {
  const a = p.strategy_summary.trim();
  const b = p.controller_prompt.trim();
  return [a, b].filter(Boolean).join("\n\n");
}
