#!/usr/bin/env bash
# Download @anthropic-ai/claude-code from npm and unpack TypeScript from cli.js.map
# Output: external/claude-code-from-npm/package/unpacked/
# Requires: node (for unpack), npm (for pack). Network on first run.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/external/claude-code-from-npm"
VER="${CLAUDE_CODE_NPM_VERSION:-2.1.88}"
TGZ="anthropic-ai-claude-code-${VER}.tgz"

mkdir -p "$DEST"
cd "$DEST"

if [[ ! -f "$TGZ" ]]; then
  npm pack "@anthropic-ai/claude-code@${VER}"
fi

if [[ ! -d package ]]; then
  tar -xzf "$TGZ"
fi

cd package
if [[ ! -f cli.js.map ]]; then
  echo "error: cli.js.map missing in $DEST/package" >&2
  exit 1
fi

cat > unpack.mjs <<'UNPACK'
import { readFileSync, writeFileSync, mkdirSync } from "fs";
import { dirname, join } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const mapFile = join(__dirname, "cli.js.map");
const outDir = join(__dirname, "unpacked");

const map = JSON.parse(readFileSync(mapFile, "utf-8"));
const sources = map.sources || [];
const contents = map.sourcesContent || [];
let written = 0;
let skipped = 0;
for (let i = 0; i < sources.length; i++) {
  const src = sources[i];
  const content = contents[i];
  if (content == null) {
    skipped++;
    continue;
  }
  const outPath = join(outDir, src.replace(/^\.\.\//g, ""));
  mkdirSync(dirname(outPath), { recursive: true });
  writeFileSync(outPath, content);
  written++;
}
console.log(`Wrote ${written} files to ${outDir}` + (skipped ? `; skipped ${skipped}` : ""));
UNPACK

node unpack.mjs
echo "Done. Sources: $DEST/package/unpacked/"
