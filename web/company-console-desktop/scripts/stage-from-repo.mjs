#!/usr/bin/env node
/**
 * Copies release `hsm_console` and Next standalone app into `staged/` for electron-builder.
 *
 * Prereqs (from repo root):
 *   cargo build --release -p hyper-stigmergy --bin hsm_console
 *   cd web/company-console && npm install && npm run build
 *
 * Merges `public` and `.next/static` into the staged standalone tree (does not modify your build dir).
 */
import * as fs from "fs";
import * as path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const desktopRoot = path.resolve(__dirname, "..");
const repoRoot = path.resolve(desktopRoot, "..", "..");
const staged = path.join(desktopRoot, "staged");
const cc = path.join(repoRoot, "web", "company-console");
const standaloneSrc = path.join(cc, ".next", "standalone");

const binSrc = path.join(repoRoot, "target", "release", "hsm_console");
const binDst = path.join(staged, "hsm_console");
const uiDst = path.join(staged, "ui");

function rmrf(p) {
  if (fs.existsSync(p)) fs.rmSync(p, { recursive: true, force: true });
}

function cpRecursive(src, dst) {
  const st = fs.statSync(src);
  if (st.isDirectory()) {
    fs.mkdirSync(dst, { recursive: true });
    for (const name of fs.readdirSync(src)) {
      cpRecursive(path.join(src, name), path.join(dst, name));
    }
  } else {
    fs.mkdirSync(path.dirname(dst), { recursive: true });
    fs.copyFileSync(src, dst);
  }
}

if (!fs.existsSync(binSrc)) {
  console.error("Missing:", binSrc);
  console.error("Run: cargo build --release -p hyper-stigmergy --bin hsm_console");
  process.exit(1);
}

if (!fs.existsSync(path.join(standaloneSrc, "server.js"))) {
  console.error("Missing:", path.join(standaloneSrc, "server.js"));
  console.error("Run: cd web/company-console && npm run build");
  process.exit(1);
}

rmrf(staged);
fs.mkdirSync(staged, { recursive: true });
fs.copyFileSync(binSrc, binDst);
fs.chmodSync(binDst, 0o755);
cpRecursive(standaloneSrc, uiDst);

if (fs.existsSync(path.join(cc, "public"))) {
  cpRecursive(path.join(cc, "public"), path.join(uiDst, "public"));
}
if (fs.existsSync(path.join(cc, ".next", "static"))) {
  fs.mkdirSync(path.join(uiDst, ".next"), { recursive: true });
  cpRecursive(path.join(cc, ".next", "static"), path.join(uiDst, ".next", "static"));
}

console.log("Staged:", binDst);
console.log("Staged UI (standalone):", uiDst);
console.log("Done.");
