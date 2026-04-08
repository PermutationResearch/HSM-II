#!/usr/bin/env node
/**
 * Next `output: "standalone"` does not copy `.next/static` or `public` into `.next/standalone/`.
 * Without this, `node .next/standalone/server.js` serves HTML with 404 on `/_next/static/*` → no Tailwind/CSS.
 * @see https://nextjs.org/docs/app/api-reference/config/next-config-js/output
 */
import * as fs from "fs";
import * as path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, "..");
const standalone = path.join(root, ".next", "standalone");
const serverJs = path.join(standalone, "server.js");

if (!fs.existsSync(serverJs)) {
  console.error("copy-standalone-assets: missing", serverJs, "— run `npm run build` first");
  process.exit(1);
}

function cpDir(src, dst) {
  if (!fs.existsSync(src)) return;
  fs.mkdirSync(path.dirname(dst), { recursive: true });
  fs.cpSync(src, dst, { recursive: true });
}

const staticSrc = path.join(root, ".next", "static");
const staticDst = path.join(standalone, ".next", "static");
const publicSrc = path.join(root, "public");
const publicDst = path.join(standalone, "public");

if (fs.existsSync(staticSrc)) {
  fs.rmSync(staticDst, { recursive: true, force: true });
  cpDir(staticSrc, staticDst);
  console.log("copy-standalone-assets: .next/static → .next/standalone/.next/static");
} else {
  console.warn("copy-standalone-assets: no .next/static (build may be incomplete)");
}

if (fs.existsSync(publicSrc)) {
  fs.rmSync(publicDst, { recursive: true, force: true });
  cpDir(publicSrc, publicDst);
  console.log("copy-standalone-assets: public → .next/standalone/public");
}
