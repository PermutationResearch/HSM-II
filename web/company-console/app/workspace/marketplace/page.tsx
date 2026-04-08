"use client";

import { useCallback, useState } from "react";
import { PackMarketplacePanel } from "@/app/components/PackMarketplacePanel";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { createFromCatalogItem } from "@/app/lib/create-from-catalog";
import { useCompaniesShCatalog, type CompaniesShItem } from "@/ui/src/hooks/useCompaniesShCatalog";

export default function WorkspaceMarketplacePage() {
  const companiesSh = useCompaniesShCatalog();
  const { apiBase, setCompanyId, companies, postgresConfigured, refreshWorkspace } = useWorkspace();
  const [coErr, setCoErr] = useState<string | null>(null);
  const [packOk, setPackOk] = useState<string | null>(null);

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

  return (
    <div className="space-y-4">
      <div>
        <p className="pc-page-eyebrow">Directory</p>
        <h1 className="pc-page-title">Pack marketplace</h1>
        <p className="pc-page-desc">
          Same catalog and install flow as the legacy console — browse templates, install packs, and add workspaces.
        </p>
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
