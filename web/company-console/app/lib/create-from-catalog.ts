import {
  findCompanyByPackFolder,
  findExistingCompanyForCatalogPack,
  isPaperclipPack,
  slugBaseFromCatalogItem,
  type CompaniesShItem,
} from "@/ui/src/hooks/useCompaniesShCatalog";

export type CatalogCompanyLookup = {
  id: string;
  slug: string;
  display_name: string;
  hsmii_home?: string | null;
};

export type CreateFromCatalogParams = {
  apiBase: string;
  postgresConfigured: boolean;
  item: CompaniesShItem;
  setError: (msg: string | null) => void;
  setPackImportOk: (msg: string | null) => void;
  /** Select workspace and reload company-scoped data */
  selectCompany: (id: string) => Promise<void>;
  /** Legacy console: jump to Company → Team after Paperclip import */
  afterPaperclipTeamOpen?: () => void;
};

/**
 * Install / link a companies.sh catalog pack — shared by legacy `/` and `/workspace/marketplace`.
 */
export async function createFromCatalogItem(p: CreateFromCatalogParams): Promise<void> {
  const { apiBase, postgresConfigured, item, setError, setPackImportOk, selectCompany, afterPaperclipTeamOpen } = p;

  if (!postgresConfigured) {
    setError("Set HSM_COMPANY_OS_DATABASE_URL and restart hsm_console to add companies.");
    return;
  }
  setError(null);
  setPackImportOk(null);

  const paperclip = isPaperclipPack(item);
  const repo = (item.repo ?? "").trim();
  const packSlug = (item.slug ?? "").trim();
  const base = slugBaseFromCatalogItem(item);

  let freshCompanies: CatalogCompanyLookup[] = [];
  try {
    const lr = await fetch(`${apiBase}/api/company/companies`);
    if (!lr.ok) throw new Error(`companies ${lr.status}`);
    const lj = (await lr.json()) as { companies?: CatalogCompanyLookup[] };
    freshCompanies = lj.companies ?? [];
  } catch (e) {
    setError(e instanceof Error ? e.message : String(e));
    return;
  }

  const existingRow =
    findExistingCompanyForCatalogPack(freshCompanies, base) ??
    findCompanyByPackFolder(freshCompanies, packSlug);

  const runInstall = async (): Promise<{ home: string | null; warning: string | null }> => {
    if (!repo || !packSlug) return { home: null, warning: null };
    try {
      const ir = await fetch("/api/companies-sh/install", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ repo, slug: packSlug }),
      });
      const raw = await ir.text();
      let ij = {} as {
        skipped?: boolean;
        hsmii_home?: string | null;
        warning?: string;
        error?: string;
      };
      if (raw.trim()) {
        try {
          ij = JSON.parse(raw) as typeof ij;
        } catch {
          return { home: null, warning: raw.slice(0, 400) || `Pack install HTTP ${ir.status}` };
        }
      }
      if (!ir.ok) {
        return { home: null, warning: ij.error ?? `Pack install HTTP ${ir.status}` };
      }
      if (typeof ij.hsmii_home === "string" && ij.hsmii_home.length > 0) {
        return { home: ij.hsmii_home, warning: null };
      }
      return { home: null, warning: ij.warning ?? null };
    } catch (e) {
      return { home: null, warning: e instanceof Error ? e.message : String(e) };
    }
  };

  const runImport = async (cid: string): Promise<boolean> => {
    const title = paperclip ? "Paperclip template" : "Pack";
    try {
      const ir = await fetch(`${apiBase}/api/company/companies/${cid}/import-paperclip-home`, {
        method: "POST",
      });
      const raw = await ir.text();
      let ij = {} as {
        error?: string;
        agents_inserted?: number;
        agents_skipped_existing?: number;
        skills_saved?: number;
      };
      if (raw.trim()) {
        try {
          ij = JSON.parse(raw) as typeof ij;
        } catch {
          setPackImportOk(null);
          setError(
            !ir.ok
              ? `${raw.slice(0, 400)} (${title})`
              : `${title}: import returned non-JSON (proxy or server error).`,
          );
          return false;
        }
      }
      if (!ir.ok) {
        setPackImportOk(null);
        setError(
          typeof ij.error === "string"
            ? `${ij.error} (${title}: hsm_console must be running; pack files must exist at hsmii_home on that host.)`
            : `${title} import failed (${ir.status})`,
        );
        return false;
      }
      setError(null);
      const inserted = ij.agents_inserted ?? 0;
      const skipped = ij.agents_skipped_existing ?? 0;
      const skills = ij.skills_saved ?? 0;
      const bits: string[] = [];
      if (inserted > 0) {
        bits.push(`${inserted} new agent${inserted === 1 ? "" : "s"} added to Team & setup`);
      }
      if (skipped > 0) {
        bits.push(`${skipped} agent${skipped === 1 ? "" : "s"} already in roster`);
      }
      if (skills > 0) {
        bits.push(`${skills} skill${skills === 1 ? "" : "s"} saved as importable templates`);
      }
      if (bits.length === 0) {
        setPackImportOk(
          paperclip
            ? `${title}: roster up to date. Skills are saved as importable templates.`
            : `${title}: synced from disk (no new agents).`,
        );
      } else {
        setPackImportOk(`${title}: ${bits.join(". ")}.`);
      }
      return true;
    } catch (e) {
      setPackImportOk(null);
      setError(e instanceof Error ? e.message : String(e));
      return false;
    }
  };

  if (existingRow) {
    let home = (existingRow.hsmii_home ?? "").trim();
    if (!home && repo && packSlug) {
      const { home: installed, warning } = await runInstall();
      if (warning && !installed) {
        setError(warning);
        return;
      }
      if (installed) {
        const pr = await fetch(`${apiBase}/api/company/companies/${existingRow.id}`, {
          method: "PATCH",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ hsmii_home: installed }),
        });
        const pRaw = await pr.text();
        let pj = {} as { error?: string };
        if (pRaw.trim()) {
          try {
            pj = JSON.parse(pRaw) as typeof pj;
          } catch {
            setError(pRaw.slice(0, 280) || `PATCH company ${pr.status}`);
            await selectCompany(existingRow.id);
            return;
          }
        }
        if (!pr.ok) {
          setError(pj.error ?? `PATCH company ${pr.status}`);
          await selectCompany(existingRow.id);
          return;
        }
        home = installed;
      }
    }
    let paperclipImportOk = false;
    if (home) {
      paperclipImportOk = await runImport(existingRow.id);
    } else {
      setError(
        paperclip
          ? "Paperclip template needs files on this machine first. Set HSM_COMPANY_PACK_INSTALL_ROOT on the Next.js server, then pick the template again: we run npx companies.sh add, set hsmii_home, then import every agents/*/AGENTS.md and index skills/*/SKILL.md into Company OS."
          : "No local pack folder is linked yet. Set HSM_COMPANY_PACK_INSTALL_ROOT on the Next.js server, then use this pack again so `npx companies.sh add` can materialize `agents/` and `skills/` on disk. Import does not clone from GitHub by itself.",
      );
      return;
    }
    await selectCompany(existingRow.id);
    if (paperclip && paperclipImportOk) {
      afterPaperclipTeamOpen?.();
    }
    return;
  }

  const { home: hsmii_home, warning: installWarn } = await runInstall();
  if (installWarn && !hsmii_home) {
    setError(installWarn);
    return;
  }

  const display_name = item.name.trim() || base;
  let slug = base;
  for (let i = 0; i < 8; i++) {
    const r = await fetch(`${apiBase}/api/company/companies`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        slug,
        display_name,
        hsmii_home: hsmii_home ?? undefined,
      }),
    });
    const j = (await r.json()) as {
      company?: { id: string; hsmii_home?: string | null };
      error?: string;
    };
    if (r.ok && j.company?.id) {
      const cid = j.company.id;
      const home = (j.company.hsmii_home ?? "").trim();
      let paperclipImportOk = false;
      if (home) {
        paperclipImportOk = await runImport(cid);
      } else if (!installWarn) {
        setError(
          paperclip
            ? "Paperclip: workspace created but no pack folder yet. Set HSM_COMPANY_PACK_INSTALL_ROOT on the Next.js host and pick this template again to install files and import agents + skills."
            : "Workspace created without a pack path. Set HSM_COMPANY_PACK_INSTALL_ROOT on the Next.js server and add this pack again to install files, then import will load agents and skills.",
        );
      }
      await selectCompany(cid);
      if (paperclip && paperclipImportOk) {
        afterPaperclipTeamOpen?.();
      }
      return;
    }
    if (r.status === 409) {
      slug = `${base}-${i + 2}`;
      continue;
    }
    setError(j.error ?? `HTTP ${r.status}`);
    return;
  }
  setError("Could not create company (slug conflict).");
}
