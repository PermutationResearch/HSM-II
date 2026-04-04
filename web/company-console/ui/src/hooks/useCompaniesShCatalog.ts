"use client";

import { useEffect, useState } from "react";

/** Shape from https://companies.sh/api/companies (includes Paperclip agent-company packs). */
export type CompaniesShItem = {
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

export function companiesShInstallPath(item: CompaniesShItem): string {
  return `${item.repo}/${item.slug}`.replace(/\/+/g, "/");
}

/** True for packs from [paperclipai/companies](https://github.com/paperclipai/companies) (440+ agents across 16 templates). */
export function isPaperclipPack(item: CompaniesShItem): boolean {
  const repo = (item.repo ?? "").toLowerCase().replace(/\s+/g, "");
  if (repo === "paperclipai/companies" || repo.endsWith("/paperclipai/companies")) return true;
  if (repo.includes("paperclipai/companies")) return true;
  const ts = item.techStack;
  if (Array.isArray(ts)) {
    return ts.some((x) => String(x).toLowerCase() === "paperclip");
  }
  return false;
}

/** Same slug normalization as Company OS POST when adding from the directory. */
export function slugBaseFromCatalogItem(item: CompaniesShItem): string {
  let base = item.slug
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/-+/g, "-")
    .replace(/^-|-$/g, "");
  if (!base) base = "company";
  return base;
}

/** Minimal company row for matching directory packs to local workspaces */
export type CatalogCompanyLookup = {
  id: string;
  slug: string;
  hsmii_home?: string | null;
};

/**
 * Find a workspace already created from this pack (first click used slug `base`, retries use `base-2`, `base-3`, …).
 */
export function findExistingCompanyForCatalogPack(
  companies: CatalogCompanyLookup[] | undefined,
  base: string
): CatalogCompanyLookup | undefined {
  const rows = Array.isArray(companies) ? companies : [];
  const escaped = base.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const re = new RegExp(`^${escaped}(-\\d+)?$`);
  const matches = rows.filter((c) => re.test(c.slug));
  if (matches.length === 0) return undefined;
  return matches.sort((a, b) => {
    if (a.slug === base) return -1;
    if (b.slug === base) return 1;
    const sa = a.slug === base ? 0 : parseInt(a.slug.slice(base.length + 1), 10) || 999;
    const sb = b.slug === base ? 0 : parseInt(b.slug.slice(base.length + 1), 10) || 999;
    return sa - sb;
  })[0];
}

/** Match by installed pack folder name in hsmii_home when slug was customized. */
export function findCompanyByPackFolder(
  companies: CatalogCompanyLookup[] | undefined,
  packSlug: string
): CatalogCompanyLookup | undefined {
  const rows = Array.isArray(companies) ? companies : [];
  const s = packSlug.trim().toLowerCase();
  if (!s) return undefined;
  return rows.find((c) => {
    const h = (c.hsmii_home ?? "").trim().toLowerCase().replace(/\\/g, "/");
    if (!h) return false;
    const segs = h.split("/").filter(Boolean);
    return segs.some((seg) => seg === s);
  });
}

export function useCompaniesShCatalog() {
  const [items, setItems] = useState<CompaniesShItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      setLoading(true);
      setError(null);
      try {
        const r = await fetch("/api/companies-sh");
        const j = (await r.json()) as { items?: CompaniesShItem[]; error?: string };
        if (!r.ok) {
          throw new Error(j.error ?? `HTTP ${r.status}`);
        }
        if (!cancelled) {
          setItems(Array.isArray(j.items) ? j.items : []);
        }
      } catch (e) {
        if (!cancelled) {
          setError(e instanceof Error ? e.message : "Failed to load directory");
          setItems([]);
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return { items, loading, error };
}
