import { NextResponse } from "next/server";

/** Proxy [companies.sh](https://companies.sh/) open directory JSON (avoids browser CORS),
 *  then merge local HSM companies so custom packs (like apex-systems) also appear. */
export const dynamic = "force-dynamic";

const HSM_CONSOLE = process.env.HSM_CONSOLE_URL?.trim() || "http://127.0.0.1:3847";

type CatalogItem = {
  slug: string;
  name: string;
  tagline?: string;
  description?: string;
  repo: string;
  installs?: string;
  website?: string;
  category?: string;
  techStack?: string[];
  githubStars?: string;
  founded?: string;
};

export async function GET() {
  /* ── 1. Fetch the public companies.sh directory ── */
  let catalogItems: CatalogItem[] = [];
  try {
    const r = await fetch("https://companies.sh/api/companies", {
      headers: { Accept: "application/json" },
    });
    if (r.ok) {
      const data = await r.json();
      catalogItems = Array.isArray(data) ? data : (data as { items?: CatalogItem[] }).items ?? [];
    }
  } catch {
    /* catalog unavailable — continue with local-only */
  }

  /* ── 2. Fetch local HSM console companies ── */
  try {
    const r = await fetch(`${HSM_CONSOLE}/api/company/companies`, {
      headers: { Accept: "application/json" },
    });
    if (r.ok) {
      const data = (await r.json()) as { companies?: { slug: string; display_name: string; hsmii_home?: string }[] };
      const localCompanies = data.companies ?? [];

      /* Build a set of slugs already in the catalog */
      const existingSlugs = new Set(catalogItems.map((i) => i.slug.toLowerCase()));

      /* Add local companies that don't already exist in the catalog */
      for (const co of localCompanies) {
        const slug = co.slug.toLowerCase();
        if (existingSlugs.has(slug)) continue;
        /* Skip internal/utility slugs */
        if (slug === "package" || slug === "test") continue;
        /* Skip duplicate gstack variants */
        if (/^gstack-\d+$/.test(slug)) continue;

        catalogItems.push({
          slug: co.slug,
          name: co.display_name || co.slug,
          tagline: "Local company pack",
          description: co.hsmii_home ? `Pack: ${co.hsmii_home}` : undefined,
          repo: co.hsmii_home ?? "local",
          category: "local",
          techStack: ["paperclip"],
        });
      }
    }
  } catch {
    /* HSM console unavailable — return catalog-only */
  }

  return NextResponse.json({ items: catalogItems });
}
