import { promises as fs } from "fs";
import path from "path";
import { spawn } from "child_process";

import { NextResponse } from "next/server";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const CONSOLE_UPSTREAM = (process.env.HSM_CONSOLE_URL ?? "http://127.0.0.1:3847").replace(/\/+$/, "");

type RunRequest = {
  companies?: Array<{
    company_id: string;
    company_pack?: string;
  }>;
  model?: string;
  config?: string;
  seed?: string;
  timeout_sec?: number;
};

type CompanyYcBenchProfile = {
  company_id: string;
  slug: string;
  display_name: string;
  controller_prompt: string;
  benchmark_spec: {
    labels?: Record<string, unknown>;
    setup_commands?: string[][];
    command?: string[];
    cwd_hint?: string;
  };
};

type ExternalBenchmarkResult = {
  name: string;
  labels: Record<string, unknown>;
  exit_code: number | null;
  passed: boolean;
  timed_out: boolean;
  score: number;
  setup_commands_run: number;
  failed_phase: string | null;
  stdout_tail: string;
  stderr_tail: string;
};

function repoRunsRoot(): string {
  // cwd is web/company-console when Next runs this route → ../../runs = repo runs/
  return process.env.HSM_RUNS_DIR?.trim() || path.resolve(process.cwd(), "..", "..", "runs");
}

function nowStamp(): string {
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}_${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}`;
}

function tail(text: string, maxChars: number = 8000): string {
  if (text.length <= maxChars) return text;
  return `...${text.slice(text.length - maxChars)}`;
}

function safeSlug(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/-+/g, "-")
    .replace(/^-|-$/g, "") || "company";
}

function resolveYcBenchDir(): string {
  const configured = process.env.HSM_YC_BENCH_DIR?.trim() || process.env.YC_BENCH_DIR?.trim();
  if (!configured) {
    throw new Error("Set HSM_YC_BENCH_DIR (or YC_BENCH_DIR) to your local yc-bench checkout before running marketplace benchmarks.");
  }
  return configured;
}

function resolveTimeoutSec(input?: number): number {
  if (typeof input === "number" && Number.isFinite(input) && input > 0) return input;
  const envValue = Number(process.env.HSM_YC_BENCH_TIMEOUT_SEC ?? "");
  if (Number.isFinite(envValue) && envValue > 0) return envValue;
  return 7200;
}

function resolveDefaultCommand(model: string, seed: string, config: string): string[] {
  return ["uv", "run", "yc-bench", "run", "--model", model, "--seed", seed, "--config", config];
}

function parseEnvJson<T>(key: string): T | null {
  const raw = process.env[key]?.trim();
  if (!raw) return null;
  try {
    return JSON.parse(raw) as T;
  } catch {
    throw new Error(`${key} must be valid JSON.`);
  }
}

function applyCommandOverrides(command: string[], model: string, seed: string, config: string): string[] {
  const next = [...command];
  const assignFlag = (flag: string, value: string) => {
    const index = next.indexOf(flag);
    if (index >= 0 && index + 1 < next.length) {
      next[index + 1] = value;
      return;
    }
    next.push(flag, value);
  };
  const modelPlaceholder = next.indexOf("YOUR_MODEL");
  if (modelPlaceholder >= 0) next[modelPlaceholder] = model;
  assignFlag("--model", model);
  assignFlag("--seed", seed);
  assignFlag("--config", config);
  return next;
}

async function fetchProfile(companyId: string): Promise<CompanyYcBenchProfile> {
  const response = await fetch(`${CONSOLE_UPSTREAM}/api/company/companies/${companyId}/yc-bench-profile`, {
    headers: { Accept: "application/json" },
    cache: "no-store",
  });
  const payload = (await response.json()) as { profile?: CompanyYcBenchProfile; error?: string };
  if (!response.ok || !payload.profile) {
    throw new Error(payload.error ?? `Failed to load YC-Bench profile for company ${companyId}`);
  }
  return payload.profile;
}

async function runCommand(command: string[], cwd: string, env: NodeJS.ProcessEnv, timeoutMs: number) {
  if (!command.length) throw new Error("benchmark command is empty");
  return await new Promise<{
    stdout: string;
    stderr: string;
    exitCode: number | null;
    timedOut: boolean;
    success: boolean;
  }>((resolve, reject) => {
    const child = spawn(command[0], command.slice(1), {
      cwd,
      env,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    let timedOut = false;

    child.stdout.on("data", (chunk) => {
      stdout += chunk.toString();
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString();
    });
    child.on("error", reject);

    const timeout = setTimeout(() => {
      timedOut = true;
      child.kill("SIGKILL");
    }, timeoutMs);

    child.on("close", (code) => {
      clearTimeout(timeout);
      resolve({
        stdout,
        stderr,
        exitCode: code,
        timedOut,
        success: !timedOut && code === 0,
      });
    });
  });
}

function deriveScore(stdout: string, success: boolean, timedOut: boolean): { passed: boolean; score: number } {
  if (timedOut) return { passed: false, score: 0 };
  try {
    const parsed = JSON.parse(stdout) as { pass?: boolean; passed?: boolean; score?: number };
    const passed = parsed.pass ?? parsed.passed ?? success;
    const score = typeof parsed.score === "number" && Number.isFinite(parsed.score) ? parsed.score : passed ? 1 : 0;
    return { passed, score };
  } catch {
    return { passed: success, score: success ? 1 : 0 };
  }
}

async function executeBenchmark(
  profile: CompanyYcBenchProfile,
  companyPack: string,
  model: string,
  seed: string,
  config: string,
  timeoutSec: number,
  ycBenchDir: string,
  promptDir: string
): Promise<ExternalBenchmarkResult> {
  const labels = {
    ...(profile.benchmark_spec.labels ?? {}),
    benchmark: "yc_bench",
    company_pack: companyPack,
    marketplace_slug: companyPack,
    workspace_slug: profile.slug,
    workspace_name: profile.display_name,
  };
  const setupCommands =
    parseEnvJson<string[][]>("HSM_YC_BENCH_SETUP_JSON") ??
    profile.benchmark_spec.setup_commands ??
    [["uv", "sync"]];
  const baseCommand =
    parseEnvJson<string[]>("HSM_YC_BENCH_COMMAND_JSON") ??
    profile.benchmark_spec.command ??
    resolveDefaultCommand(model, seed, config);
  const command = applyCommandOverrides(baseCommand, model, seed, config);

  const promptPath = path.join(promptDir, `${safeSlug(companyPack)}-${safeSlug(profile.slug)}-${Date.now()}.md`);
  await fs.writeFile(promptPath, profile.controller_prompt, "utf8");

  const env: NodeJS.ProcessEnv = {
    ...process.env,
    HSM_YC_BENCH_CONTROLLER_PROMPT: profile.controller_prompt,
    HSM_YC_BENCH_CONTROLLER_PROMPT_FILE: promptPath,
    HSM_YC_BENCH_COMPANY_ID: profile.company_id,
    HSM_YC_BENCH_WORKSPACE_SLUG: profile.slug,
    HSM_YC_BENCH_WORKSPACE_NAME: profile.display_name,
    HSM_YC_BENCH_MARKETPLACE_SLUG: companyPack,
    HSM_YC_BENCH_MODEL: model,
    HSM_YC_BENCH_CONFIG: config,
    HSM_YC_BENCH_SEED: seed,
  };

  let stdout = "";
  let stderr = "";
  let setupCommandsRun = 0;

  for (let i = 0; i < setupCommands.length; i += 1) {
    const setup = setupCommands[i] ?? [];
    const run = await runCommand(setup, ycBenchDir, env, timeoutSec * 1000);
    stdout += run.stdout ? `\n== setup[${i}] :: ${setup.join(" ")} ==\n${run.stdout.trim()}\n` : "";
    stderr += run.stderr ? `\n== setup[${i}] :: ${setup.join(" ")} ==\n${run.stderr.trim()}\n` : "";
    if (!run.success || run.timedOut) {
      return {
        name: `${companyPack}_yc_bench_${config}_seed${seed}`,
        labels,
        exit_code: run.exitCode,
        passed: false,
        timed_out: run.timedOut,
        score: 0,
        setup_commands_run: setupCommandsRun,
        failed_phase: `setup[${i}]`,
        stdout_tail: tail(stdout),
        stderr_tail: tail(stderr),
      };
    }
    setupCommandsRun += 1;
  }

  const mainRun = await runCommand(command, ycBenchDir, env, timeoutSec * 1000);
  stdout += mainRun.stdout ? `\n== main :: ${command.join(" ")} ==\n${mainRun.stdout.trim()}\n` : "";
  stderr += mainRun.stderr ? `\n== main :: ${command.join(" ")} ==\n${mainRun.stderr.trim()}\n` : "";
  const scored = deriveScore(mainRun.stdout, mainRun.success, mainRun.timedOut);

  return {
    name: `${companyPack}_yc_bench_${config}_seed${seed}`,
    labels,
    exit_code: mainRun.exitCode,
    passed: scored.passed && !mainRun.timedOut,
    timed_out: mainRun.timedOut,
    score: mainRun.timedOut ? 0 : scored.score,
    setup_commands_run: setupCommandsRun,
    failed_phase: mainRun.success || mainRun.timedOut ? (mainRun.timedOut ? "main" : null) : "main",
    stdout_tail: tail(stdout),
    stderr_tail: tail(stderr),
  };
}

export async function POST(request: Request) {
  try {
    const body = (await request.json()) as RunRequest;
    const companies = Array.isArray(body.companies) ? body.companies.filter((row) => row?.company_id) : [];
    if (companies.length === 0) {
      return NextResponse.json({ error: "Select at least one imported company workspace to benchmark." }, { status: 400 });
    }

    const model = body.model?.trim() || process.env.HSM_YC_BENCH_MODEL?.trim() || "openrouter/anthropic/claude-3.7-sonnet";
    const config = body.config?.trim() || process.env.HSM_YC_BENCH_CONFIG?.trim() || "medium";
    const seed = body.seed?.trim() || process.env.HSM_YC_BENCH_SEED?.trim() || "1";
    const timeoutSec = resolveTimeoutSec(body.timeout_sec);
    const ycBenchDir = resolveYcBenchDir();
    const runsRoot = repoRunsRoot();
    const promptDir = path.join(runsRoot, "_yc_bench_prompts");
    await fs.mkdir(promptDir, { recursive: true });

    const results: ExternalBenchmarkResult[] = [];
    for (const company of companies) {
      const profile = await fetchProfile(company.company_id);
      const companyPack = safeSlug(company.company_pack || profile.slug);
      const result = await executeBenchmark(profile, companyPack, model, seed, config, timeoutSec, ycBenchDir, promptDir);
      results.push(result);
    }

    const meanScore = results.length > 0 ? results.reduce((sum, row) => sum + row.score, 0) / results.length : 0;
    const allPassed = results.every((row) => row.passed);
    const batch = {
      results,
      mean_score: meanScore,
      all_passed: allPassed,
      stopped_early: false,
    };

    await fs.mkdir(runsRoot, { recursive: true });
    const outPath = path.join(runsRoot, `external_batch_yc_bench_${config}_seed${seed}_${nowStamp()}.json`);
    await fs.writeFile(outPath, JSON.stringify(batch, null, 2), "utf8");

    return NextResponse.json({
      ok: true,
      out_path: outPath,
      results,
      mean_score: meanScore,
      all_passed: allPassed,
    });
  } catch (e) {
    const msg = e instanceof Error ? e.message : "failed to run YC-Bench";
    return NextResponse.json({ error: msg }, { status: 500 });
  }
}
