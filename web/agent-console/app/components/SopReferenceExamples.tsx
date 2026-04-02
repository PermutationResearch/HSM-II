"use client";

import { useMemo, useState } from "react";

import { sopReferenceExamples } from "@/app/lib/sop-examples";
import type { SopExampleDocument } from "@/app/lib/sop-examples-types";
import { applyReferenceSopToCompany } from "@/app/lib/apply-reference-sop";
import { Badge } from "@/app/components/ui/badge";
import { Button } from "@/app/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/app/components/ui/card";
import { Separator } from "@/app/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/app/components/ui/tabs";

function downloadJson(ex: SopExampleDocument) {
  const blob = new Blob([JSON.stringify(ex, null, 2)], { type: "application/json" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = ex.jsonFilename;
  a.click();
  URL.revokeObjectURL(a.href);
}

function SopExampleBody({
  ex,
  apiBase,
  companyId,
  onApplied,
  setCoErr,
}: {
  ex: SopExampleDocument;
  apiBase: string | null;
  companyId: string | null;
  onApplied: (() => Promise<void>) | null;
  setCoErr: ((msg: string | null) => void) | null;
}) {
  const [working, setWorking] = useState(false);
  const canImplement = !!(apiBase && companyId && onApplied && setCoErr);

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-base font-semibold text-white">{ex.title}</h2>
          <p className="mt-1 max-w-3xl text-sm text-gray-500">{ex.summary}</p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            type="button"
            variant="default"
            size="sm"
            className="bg-primary text-primary-foreground"
            disabled={!canImplement || working}
            title={
              !companyId
                ? "Select a workspace above, then implement this SOP"
                : "Create playbook + phase tasks and seed governance templates"
            }
            onClick={() => {
              if (!apiBase || !companyId || !onApplied || !setCoErr) return;
              setWorking(true);
              setCoErr(null);
              void (async () => {
                try {
                  await applyReferenceSopToCompany({ apiBase, companyId, document: ex });
                  await onApplied();
                } catch (e) {
                  setCoErr(e instanceof Error ? e.message : String(e));
                } finally {
                  setWorking(false);
                }
              })();
            }}
          >
            {working ? "Implementing…" : "Implement in workspace"}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="border-line bg-panel"
            disabled={working}
            onClick={() => downloadJson(ex)}
          >
            Download JSON
          </Button>
        </div>
      </div>
      {!companyId ? (
        <p className="text-xs text-amber-200/90">
          Select a <strong className="font-medium">workspace</strong> (Company OS chips) to enable{" "}
          <strong className="font-medium">Implement in workspace</strong>.
        </p>
      ) : null}

      <div className="grid gap-3 md:grid-cols-2">
        {ex.phases.map((p) => (
          <Card key={p.id} className="border-line bg-panel">
            <CardHeader className="pb-2">
              <div className="flex flex-wrap items-center gap-2">
                <CardTitle className="text-sm">{p.name}</CardTitle>
                <Badge variant="secondary" className="font-mono text-[10px] uppercase">
                  {p.actor}
                </Badge>
              </div>
              <CardDescription className="font-mono text-[10px] text-gray-500">{p.id}</CardDescription>
            </CardHeader>
            <CardContent className="space-y-2 text-xs text-gray-400">
              <div>
                <span className="text-[10px] font-semibold uppercase text-gray-500">SOP logic</span>
                <p className="mt-0.5 leading-relaxed text-gray-300">{p.sop_logic}</p>
              </div>
              <div>
                <span className="text-[10px] font-semibold uppercase text-gray-500">Actions</span>
                <ul className="mt-0.5 list-inside list-disc space-y-0.5">
                  {p.actions.map((a, i) => (
                    <li key={i}>{a}</li>
                  ))}
                </ul>
              </div>
              <div>
                <span className="text-[10px] font-semibold uppercase text-gray-500">Company OS</span>
                <ul className="mt-0.5 space-y-0.5 font-mono text-[10px] text-gray-500">
                  {p.company_os.map((c, i) => (
                    <li key={i}>{c}</li>
                  ))}
                </ul>
              </div>
              {(p.resolution || p.escalation) && (
                <>
                  <Separator className="bg-line" />
                  {p.resolution ? (
                    <p>
                      <span className="text-ok">Resolution: </span>
                      {p.resolution}
                    </p>
                  ) : null}
                  {p.escalation ? (
                    <p>
                      <span className="text-warn">Escalation: </span>
                      {p.escalation}
                    </p>
                  ) : null}
                </>
              )}
            </CardContent>
          </Card>
        ))}
      </div>

      <Card className="border-line bg-panel">
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Log interaction (governance)</CardTitle>
          <CardDescription>{ex.interaction_log.description}</CardDescription>
        </CardHeader>
        <CardContent>
          <ul className="space-y-2 text-xs text-gray-400">
            {ex.interaction_log.suggested_events.map((ev, i) => (
              <li key={i} className="rounded border border-line/60 bg-ink/40 p-2">
                <span className="font-mono text-[10px] text-primary">{ev.action}</span>
                <span className="text-gray-600"> · </span>
                <span className="text-gray-500">
                  {ev.subject_type} / {ev.subject_hint}
                </span>
                <p className="mt-1 font-mono text-[10px] text-gray-500">{ev.payload_summary}</p>
              </li>
            ))}
          </ul>
        </CardContent>
      </Card>
    </div>
  );
}

export type SopReferenceExamplesProps = {
  /** `NEXT_PUBLIC_API_BASE` / console API (e.g. http://127.0.0.1:3847) */
  apiBase: string | null;
  /** Selected Company OS workspace id */
  companyId: string | null;
  /** Reload tasks / governance after implement */
  onApplied: (() => Promise<void>) | null;
  setCoErr: ((msg: string | null) => void) | null;
  /** Saved or imported SOPs for this workspace (appear after built-in references). */
  additionalExamples?: SopExampleDocument[];
};

export function SopReferenceExamples({
  apiBase = null,
  companyId = null,
  onApplied = null,
  setCoErr = null,
  additionalExamples = [],
}: Partial<SopReferenceExamplesProps> = {}) {
  const builtinIds = useMemo(() => new Set(sopReferenceExamples.map((x) => x.id)), []);
  const allExamples = useMemo(
    () => [...sopReferenceExamples, ...additionalExamples],
    [additionalExamples]
  );
  const defaultTab = sopReferenceExamples[0]?.id ?? "customer_complaint";

  return (
    <Tabs defaultValue={defaultTab} className="w-full">
      <TabsList className="mb-4 flex max-h-48 h-auto min-h-10 w-full flex-wrap justify-start gap-1 overflow-y-auto bg-muted/50 p-1">
        {allExamples.map((ex) => (
          <TabsTrigger
            key={ex.id}
            value={ex.id}
            title={ex.title}
            className="px-2.5 py-1.5 text-[11px] data-[state=active]:bg-primary data-[state=active]:text-primary-foreground"
          >
            <span className="inline-flex items-center gap-1">
              {ex.tab_label}
              {!builtinIds.has(ex.id) ? (
                <Badge variant="outline" className="px-1 py-0 text-[9px] font-normal">
                  yours
                </Badge>
              ) : null}
            </span>
          </TabsTrigger>
        ))}
      </TabsList>
      {allExamples.map((ex) => (
        <TabsContent key={ex.id} value={ex.id} className="mt-0">
          <SopExampleBody
            ex={ex}
            apiBase={apiBase}
            companyId={companyId}
            onApplied={onApplied}
            setCoErr={setCoErr}
          />
        </TabsContent>
      ))}
    </Tabs>
  );
}
