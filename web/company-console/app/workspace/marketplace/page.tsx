"use client";

import { useCallback, useState } from "react";
import { Button } from "@/app/components/ui/button";
import { PackMarketplacePanel } from "@/app/components/PackMarketplacePanel";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { companyOsUrl } from "@/app/lib/company-api-url";
import { createFromCatalogItem } from "@/app/lib/create-from-catalog";
import { useCompaniesShCatalog, type CompaniesShItem } from "@/ui/src/hooks/useCompaniesShCatalog";

export default function WorkspaceMarketplacePage() {
  const companiesSh = useCompaniesShCatalog();
  const { apiBase, companyId, setCompanyId, companies, postgresConfigured, refreshWorkspace } = useWorkspace();
  const [coErr, setCoErr] = useState<string | null>(null);
  const [packOk, setPackOk] = useState<string | null>(null);
  const [importingHermes, setImportingHermes] = useState(false);

  const createFromCatalog = useCallback(
    async (item: CompaniesShItem) => {
      setPackOk(null);
      await createFromCatalogItem({
        apiBase,
        postgresConfigured,
        item,
        setError: setCoErr,
        setPackImportOk: setPackOk,
        selectCompany: async (id) => {
          setCompanyId(id);
          await refreshWorkspace();
        },
      });
    },
    [apiBase, postgresConfigured, refreshWorkspace, setCompanyId],
  );

  const importHermesSkills = useCallback(async () => {
    if (!companyId) {
      setCoErr("Select a company first to import Hermes skills.");
      return;
    }
    setCoErr(null);
    setPackOk(null);
    setImportingHermes(true);
    try {
      const r = await fetch(
        companyOsUrl(apiBase, `/api/company/companies/${companyId}/skills/import-hermes`),
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ include_optional: true, dry_run: false }),
        },
      );
      const j = (await r.json().catch(() => ({}))) as { error?: string; imported?: number; attempted?: number };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      setPackOk(`Imported Hermes skills: ${j.imported ?? 0}/${j.attempted ?? 0} into company skill bank.`);
    } catch (e) {
      setCoErr(e instanceof Error ? e.message : String(e));
    } finally {
      setImportingHermes(false);
    }
  }, [apiBase, companyId]);

  return (
    <div className="space-y-4">
      <div>
        <p className="pc-page-eyebrow">Directory</p>
        <h1 className="pc-page-title">Pack marketplace</h1>
        <p className="pc-page-desc">
          Same catalog and install flow as the legacy console — browse templates, install packs, and add workspaces.
        </p>
        <div className="mt-3">
          <Button size="sm" variant="outline" onClick={importHermesSkills} disabled={importingHermes || !companyId}>
            {importingHermes ? "Importing Hermes skills..." : "Import Hermes skills to company"}
          </Button>
        </div>
      </div>
      {coErr ? (
        <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive-foreground">
          {coErr}
        </div>
      ) : null}
      {packOk ? (
        <div className="rounded-lg border border-emerald-900/40 bg-emerald-950/25 px-3 py-2 text-sm text-emerald-100/95">
          {packOk}
        </div>
      ) : null}
      <PackMarketplacePanel
        items={companiesSh.items}
        loading={companiesSh.loading}
        error={companiesSh.error}
        postgresConfigured={postgresConfigured}
        companies={companies.map((c) => ({
          id: c.id,
          slug: c.slug,
          hsmii_home: c.hsmii_home,
        }))}
        onCreateFromCatalog={createFromCatalog}
        setCoErr={setCoErr}
      />
    </div>
  );
}
