import fs from "fs";
import path from "path";
import { NextResponse } from "next/server";

export const runtime = "nodejs";

type GitHubTreeResponse = {
  tree?: Array<{ path?: string; type?: string }>;
};

const GITHUB_API_BASE = "https://api.github.com";
const RAW_GITHUB_BASE = "https://raw.githubusercontent.com";

function allowlistedRepo(repo: string): boolean {
  const r = repo.trim().toLowerCase();
  const allow = process.env.HSM_COMPANY_PACK_ALLOW_REPOS?.trim();
  if (!allow) return true;
  const set = new Set(
    allow
      .split(",")
      .map((s) => s.trim().toLowerCase())
      .filter(Boolean)
  );
  return set.has(r);
}

function validateSlug(slug: string): boolean {
  return /^[a-z0-9][a-z0-9_-]{0,127}$/i.test(slug.trim());
}

function validateRepo(repo: string): boolean {
  const t = repo.trim();
  if (t.includes("..") || t.startsWith("/") || t.includes("\\")) return false;
  return /^(?:[a-z0-9][a-z0-9_.-]*\/)+[a-z0-9][a-z0-9_.-]*$/i.test(t);
}

function splitRepo(repo: string): { owner: string; repoName: string } {
  const parts = repo.trim().split("/").filter(Boolean);
  if (parts.length < 2) {
    throw new Error("Repo must be owner/name");
  }
  return {
    owner: parts[0]!,
    repoName: parts[parts.length - 1]!,
  };
}

function normalizeSubpath(value: string | undefined, fallbackSlug: string): string {
  const input = (value ?? fallbackSlug).trim().replace(/^\/+|\/+$/g, "");
  if (!input || input.includes("..") || input.includes("\\")) {
    throw new Error("Invalid expected_subpath.");
  }
  return input;
}

function safeJoin(root: string, ...parts: string[]): string {
  const resolved = path.resolve(root, ...parts);
  if (!resolved.startsWith(root)) {
    throw new Error("Resolved path escapes install root.");
  }
  return resolved;
}

async function fetchGitHubJson<T>(url: string): Promise<T> {
  const response = await fetch(url, {
    headers: {
      accept: "application/vnd.github+json",
      "user-agent": "hsm-company-console",
    },
  });
  if (!response.ok) {
    throw new Error(`GitHub API ${response.status}: ${url}`);
  }
  return response.json() as Promise<T>;
}

async function fetchOptionalRaw(url: string): Promise<Buffer | null> {
  const response = await fetch(url, {
    headers: {
      "user-agent": "hsm-company-console",
    },
  });
  if (response.status === 404) return null;
  if (!response.ok) {
    throw new Error(`GitHub raw fetch ${response.status}: ${url}`);
  }
  return Buffer.from(await response.arrayBuffer());
}

function asNodeWritableBytes(bytes: Buffer): Uint8Array {
  return Uint8Array.from(bytes);
}

async function resolveGitHubRef(owner: string, repoName: string, subpath: string) {
  const refs = ["main", "master"];
  for (const ref of refs) {
    const companyBytes = await fetchOptionalRaw(
      `${RAW_GITHUB_BASE}/${owner}/${repoName}/${encodeURIComponent(ref)}/${subpath}/COMPANY.md`
    );
    if (companyBytes) {
      return { ref, companyBytes };
    }
  }
  throw new Error(`Could not find ${subpath}/COMPANY.md on main or master.`);
}

async function materializeGitHubPack(input: {
  rootResolved: string;
  repo: string;
  slug: string;
  expectedSubpath?: string;
}) {
  const { owner, repoName } = splitRepo(input.repo);
  const subpath = normalizeSubpath(input.expectedSubpath, input.slug);
  const { ref, companyBytes } = await resolveGitHubRef(owner, repoName, subpath);

  const tree = await fetchGitHubJson<GitHubTreeResponse>(
    `${GITHUB_API_BASE}/repos/${owner}/${repoName}/git/trees/${encodeURIComponent(ref)}?recursive=1`
  );
  const prefix = `${subpath}/`;
  const blobPaths = (tree.tree ?? [])
    .filter((entry) => entry.type === "blob" && typeof entry.path === "string")
    .map((entry) => entry.path as string)
    .filter((entry) => entry === `${subpath}/COMPANY.md` || entry.startsWith(prefix))
    .sort((a, b) => a.localeCompare(b));

  if (blobPaths.length === 0) {
    throw new Error(`No pack files found under ${subpath} in ${owner}/${repoName}@${ref}.`);
  }

  const installDir = safeJoin(
    input.rootResolved,
    owner.toLowerCase(),
    repoName.toLowerCase(),
    input.slug.trim().toLowerCase()
  );
  fs.rmSync(installDir, { recursive: true, force: true });
  fs.mkdirSync(installDir, { recursive: true });

  let written = 0;
  for (const fullPath of blobPaths) {
    const relative = fullPath.slice(prefix.length);
    const targetPath = safeJoin(installDir, relative);
    fs.mkdirSync(path.dirname(targetPath), { recursive: true });
    const bytes =
      fullPath === `${subpath}/COMPANY.md`
        ? companyBytes
        : await fetchOptionalRaw(`${RAW_GITHUB_BASE}/${owner}/${repoName}/${encodeURIComponent(ref)}/${fullPath}`);
    if (!bytes) {
      throw new Error(`Missing file while downloading ${fullPath}.`);
    }
    fs.writeFileSync(targetPath, asNodeWritableBytes(bytes));
    written += 1;
  }

  return {
    installDir,
    ref,
    subpath,
    written,
  };
}

/**
 * Materializes a GitHub-backed companies.sh pack into a local filesystem root so
 * Company OS can import agent and skill markdown files from disk.
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
      return NextResponse.json({
        skipped: true,
        hsmii_home: null as string | null,
        warning:
          "Pack install skipped: HSM_COMPANY_PACK_INSTALL_ROOT is not set on this Next.js server. Set it to an absolute directory where pack files should be materialized for hsm_console to import (agents/skills).",
      });
    }

    const resolvedRoot = path.resolve(root);
    if (!fs.existsSync(resolvedRoot) || !fs.statSync(resolvedRoot).isDirectory()) {
      return NextResponse.json(
        { error: `HSM_COMPANY_PACK_INSTALL_ROOT is not a directory: ${resolvedRoot}` },
        { status: 400 }
      );
    }

    const installed = await materializeGitHubPack({
      rootResolved: resolvedRoot,
      repo,
      slug,
      expectedSubpath: typeof body.expected_subpath === "string" ? body.expected_subpath : undefined,
    });

    return NextResponse.json({
      hsmii_home: installed.installDir,
      log: `materialized ${installed.written} file(s) from ${repo}/${installed.subpath} @ ${installed.ref}`,
    });
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    return NextResponse.json({ error: msg }, { status: 500 });
  }
}
