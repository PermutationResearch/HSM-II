/**
 * Syncs markdown from ../docs into this site:
 * - guide/operator-handbook.md — operator guide (VitePress + frontmatter)
 * - guide/reference/*.md — same specs as browsable pages
 * - public/llm/*.md — identical bytes for LLM fetch / curl (no frontmatter)
 * - guide/reference/index.md — auto index of reference pages
 * - public/route-to-llm.json — route.path → /llm/*.md for Copy page (MD)
 * - public/llm/docs-site-*.md — site-only pages (home, guide index, ref index) without frontmatter
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.join(__dirname, "..");
const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, "sync-manifest.json"), "utf8"));
const { githubRepo, defaultBranch, entries } = manifest;
const EDIT_BASE = `https://github.com/${githubRepo}/edit/${defaultBranch}`;

function readRepoFile(repoPath) {
  const abs = path.join(ROOT, ...repoPath.split("/"));
  if (!fs.existsSync(abs)) {
    console.error("sync-docs: missing", abs);
    process.exit(1);
  }
  return fs.readFileSync(abs, "utf8");
}

function extractTitle(md) {
  const m = md.match(/^#\s+(.+)$/m);
  return m ? m[1].trim().replace(/\*\*/g, "") : "Untitled";
}

function stripFrontmatter(md) {
  if (!md.startsWith("---")) return md;
  const lines = md.split("\n");
  if (lines[0] !== "---") return md;
  let i = 1;
  while (i < lines.length && lines[i] !== "---") i += 1;
  if (i >= lines.length) return md;
  return lines.slice(i + 1).join("\n").replace(/^\n+/, "");
}

function copySiteMd(relFromDocsSite, publicName) {
  const abs = path.join(__dirname, relFromDocsSite);
  const raw = fs.readFileSync(abs, "utf8");
  fs.writeFileSync(path.join(publicLlm, publicName), stripFrontmatter(raw), "utf8");
  console.log("sync-docs: public/llm/", publicName);
}

function frontmatter({ title, editPath }) {
  const editPattern = `${EDIT_BASE}/${editPath}`;
  return `---
title: ${JSON.stringify(title)}
outline: deep
editLink:
  pattern: ${JSON.stringify(editPattern)}
---

`;
}

const guideDir = path.join(__dirname, "guide");
const refDir = path.join(guideDir, "reference");
const publicLlm = path.join(__dirname, "public", "llm");

fs.mkdirSync(refDir, { recursive: true });
fs.mkdirSync(publicLlm, { recursive: true });

const refRows = [];

for (const ent of entries) {
  const body = readRepoFile(ent.repoPath);
  const title = (ent.title && String(ent.title).trim()) || extractTitle(body);
  const fm = frontmatter({ title, editPath: ent.repoPath });

  fs.writeFileSync(path.join(publicLlm, ent.publicFile), body, "utf8");
  console.log("sync-docs: public/llm/", ent.publicFile);

  if (ent.handbook) {
    const dst = path.join(guideDir, "operator-handbook.md");
    fs.writeFileSync(dst, fm + body, "utf8");
    console.log("sync-docs:", dst);
  } else {
    const dst = path.join(refDir, `${ent.slug}.md`);
    fs.writeFileSync(dst, fm + body, "utf8");
    console.log("sync-docs:", dst);
    refRows.push({
      label: ent.sidebarLabel || title,
      browse: `/guide/reference/${ent.slug}`,
      llm: `/llm/${ent.publicFile}`,
      repoPath: ent.repoPath,
    });
  }
}

const indexLines = [
  "# Reference",
  "",
  "These pages are **synced copies** of markdown in the HSM-II repository (`docs/…`). They are regenerated on every `npm run dev` and `npm run build` in `docs-site/`.",
  "",
  "For coding agents, use the **LLM markdown** URLs (`/llm/*.md`) — same bytes as the repo files, served as plain text from this site.",
  "",
  "| Document | Browse | Markdown for LLM |",
  "|----------|--------|------------------|",
];
for (const r of refRows) {
  indexLines.push(`| ${r.label} | [Open](${r.browse}) | [\`${r.llm}\`](${r.llm}) |`);
}
indexLines.push("");
indexLines.push("The full **operator handbook** (includes the document hub) lives at [Company OS & operator chat](../operator-handbook) and as markdown at [`/llm/HSM_II_COMPANY_OS_OPERATOR_GUIDE.md`](/llm/HSM_II_COMPANY_OS_OPERATOR_GUIDE.md).");
indexLines.push("");

const indexPath = path.join(refDir, "index.md");
fs.writeFileSync(
  indexPath,
  `---
title: Reference
outline: deep
editLink: false
---

${indexLines.join("\n")}`,
  "utf8",
);
console.log("sync-docs:", indexPath);

copySiteMd("index.md", "docs-site-home.md");
copySiteMd("guide/index.md", "docs-site-guide.md");
const refIndexBody = stripFrontmatter(fs.readFileSync(indexPath, "utf8"));
fs.writeFileSync(path.join(publicLlm, "guide-reference-index.md"), refIndexBody, "utf8");
console.log("sync-docs: public/llm/guide-reference-index.md");

function addRoutes(map, paths, llmUrl) {
  for (const p of paths) {
    map[p] = llmUrl;
  }
}

const routeToLlm = {};
addRoutes(routeToLlm, ["/", "/index.html"], "/llm/docs-site-home.md");
addRoutes(routeToLlm, ["/guide", "/guide/", "/guide/index.html"], "/llm/docs-site-guide.md");
addRoutes(routeToLlm, ["/guide/operator-handbook", "/guide/operator-handbook/", "/guide/operator-handbook.html"], "/llm/HSM_II_COMPANY_OS_OPERATOR_GUIDE.md");

for (const ent of entries) {
  if (ent.handbook) continue;
  const slug = ent.slug;
  const llm = `/llm/${ent.publicFile}`;
  addRoutes(routeToLlm, [`/guide/reference/${slug}`, `/guide/reference/${slug}.html`], llm);
}

addRoutes(routeToLlm, ["/guide/reference", "/guide/reference/", "/guide/reference/index.html"], "/llm/guide-reference-index.md");

const publicDir = path.join(__dirname, "public");
fs.mkdirSync(publicDir, { recursive: true });
fs.writeFileSync(path.join(publicDir, "route-to-llm.json"), JSON.stringify(routeToLlm, null, 2), "utf8");
console.log("sync-docs: public/route-to-llm.json");
