import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

// Absolute app root so Turbopack does not walk up to e.g. ~/package-lock.json
const __dirname = path.dirname(fileURLToPath(import.meta.url));

/**
 * Next only auto-loads `.env*` inside `web/company-console/`. Many devs keep
 * `OPENROUTER_API_KEY` in the monorepo root — merge those into `process.env` so
 * App Routes can stream operator chat without duplicating secrets.
 */
function mergeOpenRouterFromRepoRootDotenv() {
  const candidates = [
    path.join(__dirname, "..", "..", ".env"),
    path.join(__dirname, "..", "..", ".env.local"),
  ];
  for (const fp of candidates) {
    let text;
    try {
      text = fs.readFileSync(fp, "utf8");
    } catch {
      continue;
    }
    for (const line of text.split(/\r?\n/)) {
      const t = line.trim();
      if (!t || t.startsWith("#")) continue;
      const m = /^(OPENROUTER_API_KEY|OPENROUTER_API_BASE|HSM_OPENROUTER_API_KEY)\s*=\s*(.*)$/.exec(t);
      if (!m) continue;
      let val = m[2].trim();
      if (
        (val.startsWith('"') && val.endsWith('"')) ||
        (val.startsWith("'") && val.endsWith("'"))
      ) {
        val = val.slice(1, -1);
      }
      const keyName = m[1];
      if (keyName === "HSM_OPENROUTER_API_KEY") {
        if (!process.env.OPENROUTER_API_KEY) process.env.OPENROUTER_API_KEY = val;
      } else if (!process.env[keyName]) {
        process.env[keyName] = val;
      }
    }
  }
}

mergeOpenRouterFromRepoRootDotenv();

/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  output: "standalone",
  turbopack: {
    root: __dirname,
  },
  // `/api/company/*` and `/api/console/*` are proxied by App Route handlers (see app/api/company, app/api/console).
};

export default nextConfig;
