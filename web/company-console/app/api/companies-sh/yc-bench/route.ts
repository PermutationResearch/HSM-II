import { promises as fs } from "fs";
import path from "path";

import { NextResponse } from "next/server";

export const dynamic = "force-dynamic";

// Scans repo `runs/` for `external*.json` batch files (e.g. external_batch_*.json) and
// `yc_bench_result_hsm_market_*.json`. Batch rows use labels.seed; seeds 7–10 (etc.) come from
// config/external_yc_bench_seed7.json … seed10.json via `hsm_outer_loop external-batch`.

type ExternalBenchmarkResult = {
  name?: string;
  labels?: Record<string, unknown>;
  passed?: boolean;
  timed_out?: boolean;
  score?: number;
};

type ExternalBenchmarkBatchResult = {
  results?: ExternalBenchmarkResult[];
};

type HsmTimeSeries = {
  funds?: Array<{ funds_cents: number }>;
};

type HsmMarketResult = {
  terminal_reason?: string;
  time_series?: HsmTimeSeries;
};

type AggregatedCompanyScore = {
  company_pack: string;
  runs: number;
  mean_score: number;
  best_score: number;
  latest_score: number;
  pass_rate: number;
  latest_name: string | null;
  last_ran_at: string | null;
  tier: "elite" | "strong" | "promising" | "weak";
};

function normalizePackSlug(value: unknown): string {
  const raw = String(value ?? "").trim().toLowerCase().replace(/\\/g, "/");
  if (!raw) return "";
  const pieces = raw.split("/").filter(Boolean);
  return pieces[pieces.length - 1] ?? raw;
}

function repoRunsRoot(): string {
  return process.env.HSM_RUNS_DIR?.trim() || path.resolve(process.cwd(), "..", "..", "runs");
}

async function walkJsonFiles(root: string, depth = 2): Promise<string[]> {
  const out: string[] = [];

  async function visit(dir: string, remaining: number) {
    let entries;
    try {
      entries = await fs.readdir(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        if (remaining > 0) await visit(full, remaining - 1);
        continue;
      }
      if (!entry.isFile()) continue;
      const isExternal = /^external.*\.json$/i.test(entry.name);
      const isHsmMarket = /^yc_bench_result_hsm_market_.*\.json$/i.test(entry.name);
      if (!isExternal && !isHsmMarket) continue;
      out.push(full);
    }
  }

  await visit(root, depth);
  return out;
}

// Parse yc_bench_result_hsm_market_{company}_{seed}_{model}.json
// Company names use only hyphens; seed is the first integer token after them.
function parseHsmMarketFilename(name: string): { company: string; seed: number } | null {
  const prefix = "yc_bench_result_hsm_market_";
  if (!name.startsWith(prefix)) return null;
  const rest = name.slice(prefix.length).replace(/\.json$/i, "");
  const parts = rest.split("_");
  for (let i = 1; i < parts.length; i++) {
    const n = Number(parts[i]);
    if (Number.isInteger(n) && n > 0) {
      // Company parts before the seed index — rejoin with hyphens since the
      // original company slug already uses hyphens not underscores.
      return { company: parts.slice(0, i).join("-"), seed: n };
    }
  }
  return null;
}

// Score = final_funds / initial_funds (growth ratio).
// This is the same metric used in all yc-bench seed comparisons and plots.
// 1.0 = break-even, 2.0 = doubled, 3.0 = tripled. Bankrupt = 0.
// Initial funds are read from the result file's config; fallback to $200K.
function scoreFunds(finalCents: number, initialCents: number): number {
  if (initialCents <= 0) return 0;
  if (finalCents <= 0) return 0;
  return finalCents / initialCents;
}

function asResultArray(parsed: unknown): ExternalBenchmarkResult[] {
  if (!parsed || typeof parsed !== "object") return [];
  const maybeBatch = parsed as ExternalBenchmarkBatchResult;
  if (Array.isArray(maybeBatch.results)) return maybeBatch.results;
  return [parsed as ExternalBenchmarkResult];
}

// Tiers calibrated to the growth-ratio scale (1.0 = break-even).
// Elite: avg > 2.5× start  Strong: > 1.75×  Promising: > 1.25×  Weak: ≤ 1.25×
function classifyTier(meanScore: number): AggregatedCompanyScore["tier"] {
  if (meanScore >= 2.5) return "elite";
  if (meanScore >= 1.75) return "strong";
  if (meanScore >= 1.25) return "promising";
  return "weak";
}

type AggRow = {
  company_pack: string;
  runs: number;
  score_sum: number;
  best_score: number;
  latest_score: number;
  pass_count: number;
  latest_name: string | null;
  last_ran_at_ms: number;
};

function bumpSeedCount(bySeed: Map<string, number>, seedKey: string) {
  const k = seedKey.trim() || "unknown";
  bySeed.set(k, (bySeed.get(k) ?? 0) + 1);
}

function upsertAgg(
  aggregate: Map<string, AggRow>,
  companyPack: string,
  score: number,
  passed: boolean,
  fileMs: number,
  name: string | null,
) {
  const current: AggRow = aggregate.get(companyPack) ?? {
    company_pack: companyPack,
    runs: 0,
    score_sum: 0,
    best_score: 0,
    latest_score: 0,
    pass_count: 0,
    latest_name: null,
    last_ran_at_ms: 0,
  };
  current.runs += 1;
  current.score_sum += score;
  current.best_score = Math.max(current.best_score, score);
  current.pass_count += passed ? 1 : 0;
  if (fileMs >= current.last_ran_at_ms) {
    current.last_ran_at_ms = fileMs;
    current.latest_score = score;
    current.latest_name = name;
  }
  aggregate.set(companyPack, current);
}

export async function GET() {
  try {
    const runsRoot = repoRunsRoot();
    const files = await walkJsonFiles(runsRoot, 2);
    const aggregate = new Map<string, AggRow>();
    /** Count of ingested YC-Bench rows per `labels.seed` (batch JSON) or numeric seed (HSM market files). */
    const resultsBySeed = new Map<string, number>();

    for (const file of files) {
      let parsed: unknown;
      try {
        parsed = JSON.parse(await fs.readFile(file, "utf8")) as unknown;
      } catch {
        continue;
      }
      const stat = await fs.stat(file).catch(() => null);
      const fileMs = stat?.mtimeMs ?? 0;
      const basename = path.basename(file);

      // ── HSM market format ──────────────────────────────────────────────
      if (/^yc_bench_result_hsm_market_/i.test(basename)) {
        const meta = parseHsmMarketFilename(basename);
        if (!meta) continue;
        const hsmResult = parsed as HsmMarketResult;
        const funds = hsmResult?.time_series?.funds;
        if (!funds || funds.length === 0) continue; // still running
        const finalCents = funds[funds.length - 1]?.funds_cents ?? 0;
        const initialCents = (hsmResult?.time_series as Record<string, unknown>)?.config
          ? ((hsmResult.time_series as Record<string, unknown>).config as Record<string, number>).initial_funds_cents ?? 20_000_000
          : 20_000_000;
        const score = scoreFunds(finalCents, initialCents);
        const passed = finalCents > 0;
        upsertAgg(aggregate, meta.company, score, passed, fileMs, `seed-${meta.seed}`);
        bumpSeedCount(resultsBySeed, String(meta.seed));
        continue;
      }

      // ── External batch format ──────────────────────────────────────────
      for (const result of asResultArray(parsed)) {
        const labels = result.labels ?? {};
        const benchmark = String(labels.benchmark ?? "").trim().toLowerCase();
        const companyPack = normalizePackSlug(
          labels.marketplace_slug ?? labels.company_pack ?? labels.workspace_slug
        );
        if (benchmark !== "yc_bench" || !companyPack) continue;
        const score = typeof result.score === "number" && Number.isFinite(result.score) ? result.score : 0;
        const passed = !!result.passed;
        upsertAgg(aggregate, companyPack, score, passed, fileMs, result.name ?? null);
        const rawSeed = labels.seed;
        const seedKey =
          rawSeed !== undefined && rawSeed !== null && String(rawSeed).trim() !== ""
            ? String(rawSeed).trim()
            : "unknown";
        bumpSeedCount(resultsBySeed, seedKey);
      }
    }

    const scores = Object.fromEntries(
      Array.from(aggregate.entries()).map(([companyPack, row]) => {
        const meanScore = row.runs > 0 ? row.score_sum / row.runs : 0;
        const payload: AggregatedCompanyScore = {
          company_pack: companyPack,
          runs: row.runs,
          mean_score: meanScore,
          best_score: row.best_score,
          latest_score: row.latest_score,
          pass_rate: row.runs > 0 ? row.pass_count / row.runs : 0,
          latest_name: row.latest_name,
          last_ran_at: row.last_ran_at_ms > 0 ? new Date(row.last_ran_at_ms).toISOString() : null,
          tier: classifyTier(meanScore),
        };
        return [companyPack, payload];
      })
    );

    const seedEntries = [...resultsBySeed.entries()].sort(([a], [b]) => {
      const na = Number(a);
      const nb = Number(b);
      if (Number.isFinite(na) && Number.isFinite(nb) && String(na) === a && String(nb) === b) return na - nb;
      return a.localeCompare(b);
    });
    const results_by_seed = Object.fromEntries(seedEntries);

    return NextResponse.json({
      runs_root: runsRoot,
      company_scores: scores,
      results_by_seed,
    });
  } catch (e) {
    const msg = e instanceof Error ? e.message : "failed to read YC-Bench results";
    return NextResponse.json({ error: msg, company_scores: {}, results_by_seed: {} }, { status: 500 });
  }
}
