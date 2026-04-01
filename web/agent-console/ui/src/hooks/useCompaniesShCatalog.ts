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
