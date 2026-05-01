import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vitepress";

const GITHUB = "https://github.com/PermutationResearch/HSM-II";
const EDIT = `${GITHUB}/edit/main/docs-site/:path`;

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const manifest = JSON.parse(
  fs.readFileSync(path.join(__dirname, "..", "sync-manifest.json"), "utf8"),
) as {
  entries: Array<{
    handbook?: boolean;
    sidebarLabel?: string;
    slug: string;
  }>;
};

const referenceSidebarItems = [
  { text: "Overview", link: "/guide/reference/" },
  ...manifest.entries
    .filter((e) => !e.handbook)
    .map((e) => ({
      text: e.sidebarLabel ?? e.slug,
      link: `/guide/reference/${e.slug}`,
    })),
];

// GitHub Pages project site example: VITEPRESS_BASE=/HSM-II/ npm run build
function vitepressBase(): string {
  const raw = process.env.VITEPRESS_BASE?.trim();
  if (!raw || raw === "/") return "/";
  return raw.endsWith("/") ? raw : `${raw}/`;
}

/** Plain `python -m http.server` has no extensionless URL rewrites; use .html paths for web_interface bundle. */
const webStaticBundle = process.env.VITEPRESS_WEB_BUNDLE === "1";

export default defineConfig({
  base: vitepressBase(),
  // Mirrored docs keep repo-relative links (./foo, ../src/...) that are not VitePress routes.
  // Static /llm/*.md copies are checked as markdown and would also false-positive.
  ignoreDeadLinks: true,
  title: "HSM-II Docs",
  description:
    "Hyper-Stigmergic Morphogenesis II — Company OS, operator agent-chat, and the durable agent runtime.",
  lang: "en-US",
  cleanUrls: !webStaticBundle,
  lastUpdated: true,
  appearance: false,
  head: [
    ["link", { rel: "preconnect", href: "https://fonts.googleapis.com" }],
    ["link", { rel: "preconnect", href: "https://fonts.gstatic.com", crossorigin: "" }],
    [
      "link",
      {
        href: "https://fonts.googleapis.com/css2?family=Instrument+Serif:ital@0;1&family=Inter:wght@400;500;600;700&display=swap",
        rel: "stylesheet",
      },
    ],
  ],
  themeConfig: {
    logo: undefined,
    nav: [
      { text: "Documentation", link: "/guide/", activeMatch: "^/guide/" },
      { text: "Handbook", link: "/guide/operator-handbook" },
      { text: "Reference", link: "/guide/reference/", activeMatch: "^/guide/reference/" },
      { text: "GitHub", link: GITHUB },
    ],
    sidebar: {
      "/guide/reference/": [
        {
          text: "Reference (synced markdown)",
          items: referenceSidebarItems,
        },
      ],
      "/guide/": [
        {
          text: "Introduction",
          items: [
            { text: "Home", link: "/" },
            { text: "Guide overview", link: "/guide/" },
            { text: "Company OS & operator chat", link: "/guide/operator-handbook" },
            {
              text: "Document hub (raw / LLM / MCP)",
              link: "/guide/operator-handbook#document-hub",
            },
          ],
        },
        {
          text: "Reference (synced markdown)",
          items: referenceSidebarItems,
        },
      ],
    },
    socialLinks: [{ icon: "github", link: GITHUB }],
    footer: {
      message: "HSM-II documentation — built with VitePress",
      copyright: "See repository LICENSE",
    },
    editLink: {
      pattern: EDIT,
      text: "Edit on GitHub",
    },
    outline: { level: [2, 3] },
    search: { provider: "local" },
  },
});
