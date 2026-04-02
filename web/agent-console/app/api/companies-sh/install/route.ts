import { spawn } from "child_process";
import fs from "fs";
import path from "path";
import { NextResponse } from "next/server";

export const runtime = "nodejs";

/** companies.sh can take several minutes on first npx fetch */
const NPM_TIMEOUT_MS = 10 * 60 * 1000;

const DEFAULT_NPX_PACKAGE = "companies.sh";

function allowlistedRepo(repo: string): boolean {
  const r = repo.trim().toLowerCase();
  const allow = process.env.HSM_COMPANY_PACK_ALLOW_REPOS?.trim();
  if (allow) {
    const set = new Set(
      allow
        .split(",")
        .map((s) => s.trim().toLowerCase())
        .filter(Boolean)
    );
    return set.has(r);
  }
  return r === "paperclipai/companies";
}

function validateSlug(slug: string): boolean {
  return /^[a-z0-9][a-z0-9_-]{0,127}$/i.test(slug.trim());
}

/** GitHub-style owner/repo segments, no traversal */
function validateRepo(repo: string): boolean {
  const t = repo.trim();
  if (t.includes("..") || t.startsWith("/") || t.includes("\\")) return false;
  return /^(?:[a-z0-9][a-z0-9_.-]*\/)+[a-z0-9][a-z0-9_.-]*$/i.test(t);
}

function runNpxAdd(packArg: string, cwd: string): Promise<{ code: number; stdout: string; stderr: string }> {
  const pkg = process.env.HSM_COMPANIES_SH_NPX_PACKAGE?.trim() || DEFAULT_NPX_PACKAGE;
  return new Promise((resolve, reject) => {
    const isWin = process.platform === "win32";
    const cmd = isWin ? "npx.cmd" : "npx";
    const child = spawn(cmd, ["-y", pkg, "add", packArg], {
      cwd,
      env: process.env,
      shell: false,
    });
    let stdout = "";
    let stderr = "";
    const timer = setTimeout(() => {
      child.kill("SIGTERM");
      reject(new Error(`npx timed out after ${NPM_TIMEOUT_MS / 1000}s`));
    }, NPM_TIMEOUT_MS);
    child.stdout?.on("data", (d) => {
      stdout += String(d);
    });
    child.stderr?.on("data", (d) => {
      stderr += String(d);
    });
    child.on("error", (e) => {
      clearTimeout(timer);
      reject(e);
    });
    child.on("close", (code) => {
      clearTimeout(timer);
      resolve({ code: code ?? 1, stdout, stderr });
    });
  });
}

function resolveInstalledDir(
  rootResolved: string,
  repo: string,
  slug: string,
  expectSubpath: string | undefined
): string | null {
  if (expectSubpath?.trim()) {
    const rel = expectSubpath.trim().replace(/^[/\\]+/, "");
    const p = path.normalize(path.join(rootResolved, rel));
    if (!p.startsWith(rootResolved)) return null;
    try {
      if (fs.existsSync(p) && fs.statSync(p).isDirectory()) return p;
    } catch {
      return null;
    }
    return null;
  }
  const r = repo.trim().toLowerCase();
  const s = slug.trim().toLowerCase();
  const candidates = [
    path.join(rootResolved, s),
    path.join(rootResolved, ...r.split("/"), s),
    path.join(rootResolved, `${r.replace(/\//g, "-")}_${s}`),
  ].map((p) => path.resolve(p));
  for (const p of candidates) {
    if (!p.startsWith(rootResolved)) continue;
    try {
      if (fs.existsSync(p) && fs.statSync(p).isDirectory()) return p;
    } catch {
      /* continue */
    }
  }

  if (process.env.HSM_COMPANY_PACK_FALLBACK_NEWEST_DIR === "1") {
    try {
      const names = fs.readdirSync(rootResolved, { withFileTypes: true });
      const dirs = names.filter(
        (d) => d.isDirectory() && d.name !== "node_modules" && !d.name.startsWith(".")
      );
      if (dirs.length !== 1) return null;
      const only = path.join(rootResolved, dirs[0]!.name);
      const st = fs.statSync(only);
      if (st.isDirectory()) return path.resolve(only);
    } catch {
      return null;
    }
  }
  return null;
}

/**
 * Runs `npx -y companies.sh add <repo>/<slug>` under HSM_COMPANY_PACK_INSTALL_ROOT when set.
 * Returns absolute path for company.hsmii_home on success.
 */
export async function POST(req: Request) {
  try {
    const body = (await req.json()) as {
      repo?: string;
      slug?: string;
      expected_subpath?: string;
    };
    const repo = typeof body.repo === "string" ? body.repo : "";
    const slug = typeof body.slug === "string" ? body.slug : "";
    if (!validateRepo(repo)) {
      return NextResponse.json({ error: "Invalid repo format." }, { status: 400 });
    }
    if (!validateSlug(slug)) {
      return NextResponse.json({ error: "Invalid slug." }, { status: 400 });
    }
    if (!allowlistedRepo(repo)) {
      return NextResponse.json(
        {
          error:
            "Repo is not allowlisted. Default: paperclipai/companies. Set HSM_COMPANY_PACK_ALLOW_REPOS=comma,separated,owner/repos",
        },
        { status: 403 }
      );
    }

    const root = process.env.HSM_COMPANY_PACK_INSTALL_ROOT?.trim();
    if (!root) {
      return NextResponse.json({ skipped: true, hsmii_home: null as string | null });
    }

    const resolvedRoot = path.resolve(root);
    if (!fs.existsSync(resolvedRoot) || !fs.statSync(resolvedRoot).isDirectory()) {
      return NextResponse.json(
        { error: `HSM_COMPANY_PACK_INSTALL_ROOT is not a directory: ${resolvedRoot}` },
        { status: 400 }
      );
    }

    const packArg = `${repo.trim().toLowerCase()}/${slug.trim().toLowerCase()}`.replace(/\/+/g, "/");

    let result: { code: number; stdout: string; stderr: string };
    try {
      result = await runNpxAdd(packArg, resolvedRoot);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      return NextResponse.json({ hsmii_home: null, warning: msg });
    }

    const logTail = `${result.stdout}\n${result.stderr}`.trimEnd();

    if (result.code !== 0) {
      return NextResponse.json({
        hsmii_home: null,
        warning: `companies.sh exited with code ${result.code}. ${(result.stderr || result.stdout).slice(-2500)}`,
        log: logTail.slice(-12000),
      });
    }

    const installed = resolveInstalledDir(
      resolvedRoot,
      repo,
      slug,
      typeof body.expected_subpath === "string" ? body.expected_subpath : undefined
    );
    if (!installed) {
      return NextResponse.json({
        hsmii_home: null,
        warning: `npx finished but no pack directory was found under ${resolvedRoot}. Set HSM_COMPANY_PACK_FALLBACK_NEWEST_DIR=1 if the CLI drops a single new folder here, or configure expected_subpath.`,
        log: logTail.slice(-12000),
      });
    }

    return NextResponse.json({ hsmii_home: installed, log: logTail.slice(-4000) });
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    return NextResponse.json({ error: msg }, { status: 500 });
  }
}
