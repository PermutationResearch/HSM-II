/**
 * Copies VitePress build output into web_interface/docs/ for the static
 * marketing site (e.g. python3 -m http.server 4242 from web_interface/).
 * Expects VITEPRESS_BASE=/docs/ so URLs resolve under /docs/… on :4242.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const docsSiteRoot = path.join(__dirname, "..");
const dist = path.join(docsSiteRoot, ".vitepress", "dist");
const target = path.join(docsSiteRoot, "..", "web_interface", "docs");

if (!fs.existsSync(dist)) {
  console.error("copy-to-web-interface: missing dist — run vitepress build first (with VITEPRESS_BASE=/docs/).");
  process.exit(1);
}

fs.rmSync(target, { recursive: true, force: true });
fs.mkdirSync(path.dirname(target), { recursive: true });
fs.cpSync(dist, target, { recursive: true });
console.log("copy-to-web-interface:", target);
