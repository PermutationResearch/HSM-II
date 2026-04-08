"use client";

import { useEffect, useState } from "react";
import { SopComposerPanel } from "@/app/components/SopComposerPanel";
import { SopReferenceExamples } from "@/app/components/SopReferenceExamples";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { sopReferenceExamples } from "@/app/lib/sop-examples";
import type { SopExampleDocument } from "@/app/lib/sop-examples-types";
import { loadCustomSops } from "@/app/lib/sop-storage";

export default function WorkspacePlaybooksPage() {
  const { apiBase, companyId, companies, refreshWorkspace } = useWorkspace();
  const companyRow = companies.find((c) => c.id === companyId);
  const [customSops, setCustomSops] = useState<SopExampleDocument[]>([]);
  const [coErr, setCoErr] = useState<string | null>(null);

  useEffect(() => {
    if (!companyId) {
      setCustomSops([]);
      return;
    }
    setCustomSops(loadCustomSops(companyId));
  }, [companyId]);

  return (
    <div className="space-y-6">
      <div>
        <p className="pc-page-eyebrow">Procedures</p>
        <h1 className="pc-page-title">Playbooks</h1>
        <p className="pc-page-desc">
          Author SOPs, apply reference templates, and implement playbooks as tasks — scoped by{" "}
          <span className="font-mono text-xs">project</span> in Company OS, aligned with your pack{" "}
          <span className="font-mono text-xs">visions.md</span> (see{" "}
          <code className="rounded border border-white/10 px-1 font-mono text-[11px]">docs/company-os/playbooks-projects-and-visions.md</code>
          ).
        </p>
      </div>
      {coErr ? (
        <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive-foreground">
          {coErr}
        </div>
      ) : null}
      {!companyId ? (
        <p className="pc-page-desc rounded-lg border border-amber-900/40 bg-amber-950/20 px-4 py-3 text-sm text-amber-100/90">
          Select a company in the header to author SOPs and playbooks.
        </p>
      ) : (
        <>
          <SopComposerPanel
            apiBase={apiBase}
            companyId={companyId}
            contextMarkdown={companyRow?.context_markdown}
            hsmiiHome={companyRow?.hsmii_home}
            referenceExamples={sopReferenceExamples}
            onCustomSopsChanged={setCustomSops}
            onApplied={async () => {
              await refreshWorkspace();
            }}
            setCoErr={setCoErr}
          />
          <div>
            <h2 className="mb-2 text-sm font-semibold text-foreground">Reference &amp; saved library</h2>
            <p className="mb-4 max-w-3xl text-xs leading-relaxed text-muted-foreground">
              Built-in examples plus templates you saved for this workspace. Use{" "}
              <strong className="font-medium text-foreground">Implement in workspace</strong> to create playbook tasks
              and governance seeds.
            </p>
            <SopReferenceExamples
              apiBase={apiBase}
              companyId={companyId}
              onApplied={async () => {
                await refreshWorkspace();
              }}
              setCoErr={setCoErr}
              additionalExamples={customSops}
            />
          </div>
        </>
      )}
    </div>
  );
}
