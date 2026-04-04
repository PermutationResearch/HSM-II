"use client";

import { useCallback, useEffect, useMemo, useState } from "react";

import { cn } from "@/app/lib/utils";
import type { SopExampleDocument, SopGovernanceEvent, SopPhase } from "@/app/lib/sop-examples-types";
import { applyReferenceSopToCompany } from "@/app/lib/apply-reference-sop";
import {
  defaultRecommendedInteractionLog,
  emptyGovernanceEvent,
  emptyPhase,
  emptySopDocument,
  isRecommendedGovernanceLog,
  normalizeId,
  normalizeSopDocument,
  parseSopDocumentJson,
  sanitizeSopDocument,
  validateSopDocument,
} from "@/app/lib/sop-document-utils";
import { loadCustomSops, removeCustomSop, upsertCustomSop } from "@/app/lib/sop-storage";
import { Badge } from "@/app/components/ui/badge";
import { Button } from "@/app/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/app/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/app/components/ui/collapsible";
import { Input } from "@/app/components/ui/input";
import { Label } from "@/app/components/ui/label";
import { Textarea } from "@/app/components/ui/textarea";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/app/components/ui/select";
import { Separator } from "@/app/components/ui/separator";
import { ChevronDown, GitBranch, Plus, Sparkles, Trash2, Users } from "lucide-react";

function downloadJson(doc: SopExampleDocument) {
  const blob = new Blob([JSON.stringify(doc, null, 2)], { type: "application/json" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = doc.jsonFilename || `${doc.id}.json`;
  a.click();
  URL.revokeObjectURL(a.href);
}

const GOV_OFF_DESCRIPTION =
  "This playbook does not add suggested audit entries. You can still add notes manually when running it.";

function ChecklistField({
  label,
  hint,
  placeholder,
  lines,
  onChange,
  addLabel,
}: {
  label: string;
  hint?: string;
  placeholder: string;
  lines: string[];
  onChange: (next: string[]) => void;
  addLabel: string;
}) {
  const safeLines = lines.length ? lines : [""];
  const updateLine = (i: number, v: string) => {
    const base = lines.length ? [...lines] : [""];
    base[i] = v;
    onChange(base);
  };
  const removeLine = (i: number) => {
    const base = lines.length ? lines.filter((_, j) => j !== i) : [];
    onChange(base);
  };
  const addLine = () => onChange([...(lines.length ? lines : []), ""]);

  return (
    <div className="space-y-2">
      <div>
        <Label className="text-xs text-gray-200">{label}</Label>
        {hint ? <p className="mt-0.5 text-xs leading-relaxed text-gray-500">{hint}</p> : null}
      </div>
      <div className="space-y-2">
        {safeLines.map((line, i) => (
          <div key={i} className="flex gap-2">
            <Input
              value={line}
              onChange={(e) => updateLine(i, e.target.value)}
              placeholder={placeholder}
              className="flex-1 border-line bg-ink text-sm"
            />
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="h-9 w-9 shrink-0 text-gray-400 hover:text-white"
              onClick={() => removeLine(i)}
              aria-label={`Remove item ${i + 1}`}
            >
              <Trash2 className="size-4" />
            </Button>
          </div>
        ))}
        <Button type="button" variant="outline" size="sm" className="gap-1 border-line" onClick={addLine}>
          <Plus className="size-4" />
          {addLabel}
        </Button>
      </div>
    </div>
  );
}

function ActorHint({ actor }: { actor: SopPhase["actor"] }) {
  const map = {
    ai: {
      label: "Assistant-led",
      hint: "The automation or assistant does the heavy lifting here.",
      icon: Sparkles,
      className: "border-violet-500/30 bg-violet-500/10 text-violet-200",
    },
    human: {
      label: "People-led",
      hint: "A person owns this step; the assistant only helps if needed.",
      icon: Users,
      className: "border-amber-500/30 bg-amber-500/10 text-amber-100",
    },
    both: {
      label: "Together",
      hint: "People and assistant share the work (review, handoffs, or back-and-forth).",
      icon: GitBranch,
      className: "border-sky-500/30 bg-sky-500/10 text-sky-100",
    },
  } as const;
  const cfg = map[actor];
  const Icon = cfg.icon;
  return (
    <div className={cn("flex items-start gap-2 rounded-lg border px-3 py-2 text-xs", cfg.className)}>
      <Icon className="mt-0.5 size-4 shrink-0 opacity-90" aria-hidden />
      <div>
        <span className="font-medium">{cfg.label}</span>
        <p className="mt-0.5 text-[11px] leading-snug opacity-90">{cfg.hint}</p>
      </div>
    </div>
  );
}

const STEPS = [
  { id: 0, title: "Basics", blurb: "Name the playbook and when to use it." },
  { id: 1, title: "Steps", blurb: "Break the work into ordered steps with clear tasks." },
  { id: 2, title: "Audit trail", blurb: "Choose how much automatic logging to suggest." },
] as const;

type Props = {
  apiBase: string;
  companyId: string | null;
  referenceExamples: SopExampleDocument[];
  onCustomSopsChanged: (docs: SopExampleDocument[]) => void;
  onApplied: () => Promise<void>;
  setCoErr: (msg: string | null) => void;
};

export function SopComposerPanel({
  apiBase,
  companyId,
  referenceExamples,
  onCustomSopsChanged,
  onApplied,
  setCoErr,
}: Props) {
  const [draft, setDraft] = useState<SopExampleDocument>(() => emptySopDocument());
  const [savedIds, setSavedIds] = useState<string[]>([]);
  const [loadSavedId, setLoadSavedId] = useState<string>("");
  const [copyFromRefId, setCopyFromRefId] = useState<string>("");
  const [validation, setValidation] = useState<string[]>([]);
  const [working, setWorking] = useState(false);
  const [wizardStep, setWizardStep] = useState(0);

  const refreshSaved = useCallback(() => {
    if (!companyId) {
      setSavedIds([]);
      return;
    }
    const docs = loadCustomSops(companyId);
    setSavedIds(docs.map((d) => d.id));
  }, [companyId]);

  useEffect(() => {
    refreshSaved();
  }, [companyId, refreshSaved]);

  useEffect(() => {
    setDraft(emptySopDocument());
    setLoadSavedId("");
    setValidation([]);
    setWizardStep(0);
  }, [companyId]);

  const normalizedDraft = useMemo(() => {
    try {
      const n = normalizeSopDocument({
        ...draft,
        id: normalizeId(draft.id),
      });
      return sanitizeSopDocument(n);
    } catch {
      return null;
    }
  }, [draft]);

  const forbiddenIds = useMemo(
    () => new Set(referenceExamples.map((x) => normalizeId(x.id))),
    [referenceExamples]
  );

  const runValidate = useCallback(() => {
    if (!normalizedDraft) return [];
    return validateSopDocument(normalizedDraft, { forbiddenIds });
  }, [normalizedDraft, forbiddenIds]);

  const governanceMode = useMemo(() => {
    if (draft.interaction_log.suggested_events.length === 0) return "off" as const;
    if (isRecommendedGovernanceLog(draft.interaction_log)) return "recommended" as const;
    return "custom" as const;
  }, [draft.interaction_log]);

  const saveToWorkspace = useCallback(() => {
    if (!companyId) return;
    const doc = normalizedDraft;
    if (!doc) return;
    const errs = validateSopDocument(doc, { forbiddenIds });
    setValidation(errs);
    if (errs.length) return;
    const list = upsertCustomSop(companyId, doc);
    onCustomSopsChanged(list);
    refreshSaved();
    setCoErr(null);
  }, [companyId, normalizedDraft, forbiddenIds, onCustomSopsChanged, refreshSaved, setCoErr]);

  const deleteSaved = useCallback(() => {
    if (!companyId || !loadSavedId) return;
    const list = removeCustomSop(companyId, loadSavedId);
    onCustomSopsChanged(list);
    refreshSaved();
    setLoadSavedId("");
    setDraft(emptySopDocument());
    setWizardStep(0);
  }, [companyId, loadSavedId, onCustomSopsChanged, refreshSaved]);

  const implement = useCallback(() => {
    if (!companyId || !normalizedDraft) return;
    const errs = validateSopDocument(normalizedDraft, { forbiddenIds });
    setValidation(errs);
    if (errs.length) return;
    setWorking(true);
    setCoErr(null);
    void (async () => {
      try {
        await applyReferenceSopToCompany({
          apiBase,
          companyId,
          document: normalizedDraft,
        });
        await onApplied();
      } catch (e) {
        setCoErr(e instanceof Error ? e.message : String(e));
      } finally {
        setWorking(false);
      }
    })();
  }, [apiBase, companyId, normalizedDraft, forbiddenIds, onApplied, setCoErr]);

  if (!companyId) {
    return (
      <p className="mb-4 text-sm text-amber-200/90">
        Select a <strong className="font-medium">workspace</strong> to compose and save playbook templates for this company.
      </p>
    );
  }

  const updatePhase = (index: number, patch: Partial<SopPhase>) => {
    setDraft((d) => {
      const phases = [...d.phases];
      phases[index] = { ...phases[index], ...patch };
      return { ...d, phases };
    });
  };

  const addPhase = () => {
    setDraft((d) => ({
      ...d,
      phases: [...d.phases, emptyPhase(`step_${d.phases.length + 1}`)],
    }));
  };

  const removePhase = (index: number) => {
    setDraft((d) => ({
      ...d,
      phases: d.phases.filter((_, i) => i !== index),
    }));
  };

  const movePhase = (index: number, dir: -1 | 1) => {
    setDraft((d) => {
      const j = index + dir;
      if (j < 0 || j >= d.phases.length) return d;
      const phases = [...d.phases];
      [phases[index], phases[j]] = [phases[j]!, phases[index]!];
      return { ...d, phases };
    });
  };

  const updateGov = (index: number, patch: Partial<SopGovernanceEvent>) => {
    setDraft((d) => {
      const ev = [...d.interaction_log.suggested_events];
      ev[index] = { ...ev[index], ...patch };
      return {
        ...d,
        interaction_log: { ...d.interaction_log, suggested_events: ev },
      };
    });
  };

  const addGov = () => {
    setDraft((d) => ({
      ...d,
      interaction_log: {
        ...d.interaction_log,
        suggested_events: [...d.interaction_log.suggested_events, emptyGovernanceEvent()],
      },
    }));
  };

  const removeGov = (index: number) => {
    setDraft((d) => ({
      ...d,
      interaction_log: {
        ...d.interaction_log,
        suggested_events: d.interaction_log.suggested_events.filter((_, i) => i !== index),
      },
    }));
  };

  const setGovernancePreset = (mode: "recommended" | "off") => {
    if (mode === "recommended") {
      setDraft((d) => ({ ...d, interaction_log: defaultRecommendedInteractionLog() }));
    } else {
      setDraft((d) => ({
        ...d,
        interaction_log: { description: GOV_OFF_DESCRIPTION, suggested_events: [] },
      }));
    }
  };

  return (
    <div className="mb-4 space-y-4 rounded-xl border border-line bg-panel p-5 shadow-sm ring-1 ring-white/5">
      <div>
        <p className="font-mono text-[10px] font-semibold uppercase tracking-[0.12em] text-primary">Core · Playbook author</p>
        <h3 className="mt-1 text-base font-semibold tracking-tight text-white">Playbook composer</h3>
        <p className="mt-2 max-w-3xl text-sm leading-relaxed text-gray-500">
          Walk through three short screens: describe the playbook, list what happens in order, then pick how logging should
          work. Everything saves in this browser for the selected workspace; you can export JSON or implement when ready. Need
          ids, filenames, or raw audit rows? Expand <span className="text-gray-400">Technical options</span> in each section.
        </p>
      </div>

      <div className="flex flex-wrap items-end gap-3">
        <div className="space-y-1">
          <Label className="text-xs text-gray-500">Load saved template</Label>
          <Select
            value={loadSavedId || "__none__"}
            onValueChange={(v) => {
              if (v === "__none__") {
                setLoadSavedId("");
                return;
              }
              setLoadSavedId(v);
              const doc = loadCustomSops(companyId).find((x) => x.id === v);
              if (doc) setDraft({ ...doc });
            }}
          >
            <SelectTrigger className="w-[220px] border-line bg-ink text-sm">
              <SelectValue placeholder="—" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="__none__">— New draft —</SelectItem>
              {savedIds.map((id) => (
                <SelectItem key={id} value={id}>
                  {id}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-1">
          <Label className="text-xs text-gray-500">Start from a reference</Label>
          <Select
            value={copyFromRefId || "__none__"}
            onValueChange={(v) => {
              setCopyFromRefId(v);
              if (v === "__none__") return;
              const src = referenceExamples.find((x) => x.id === v);
              if (!src) return;
              const clone = JSON.parse(JSON.stringify(src)) as SopExampleDocument;
              clone.id = `${normalizeId(clone.id)}_copy_${Date.now().toString(36)}`;
              clone.tab_label = `${clone.tab_label} (copy)`;
              clone.jsonFilename = `${clone.id}.json`;
              setDraft(clone);
            }}
          >
            <SelectTrigger className="w-[220px] border-line bg-ink text-sm">
              <SelectValue placeholder="—" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="__none__">—</SelectItem>
              {referenceExamples.map((ex) => (
                <SelectItem key={ex.id} value={ex.id}>
                  {ex.tab_label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="border-line"
          onClick={() => {
            setDraft(emptySopDocument());
            setWizardStep(0);
          }}
        >
          New draft
        </Button>
        <label className="cursor-pointer text-sm text-accent hover:underline">
          Import JSON…
          <input
            type="file"
            accept="application/json,.json"
            className="hidden"
            onChange={(e) => {
              const f = e.target.files?.[0];
              e.target.value = "";
              if (!f) return;
              void f.text().then((t) => {
                try {
                  setDraft(parseSopDocumentJson(t));
                  setValidation([]);
                } catch (err) {
                  setCoErr(err instanceof Error ? err.message : String(err));
                }
              });
            }}
          />
        </label>
      </div>

      <div className="flex flex-wrap gap-2">
        {STEPS.map((s, i) => (
          <button
            key={s.id}
            type="button"
            onClick={() => setWizardStep(i)}
            className={cn(
              "rounded-md border px-3 py-2 text-left text-xs transition-colors",
              wizardStep === i
                ? "border-primary/60 bg-primary/15 text-white"
                : "border-line bg-black/20 text-gray-400 hover:border-line-strong hover:text-gray-200"
            )}
          >
            <span className="font-semibold text-gray-200">
              {i + 1}. {s.title}
            </span>
            <span className="mt-0.5 block text-[11px] font-normal text-gray-500">{s.blurb}</span>
          </button>
        ))}
      </div>

      <Separator className="bg-line" />

      {wizardStep === 0 ? (
        <div className="space-y-4">
          <div className="grid gap-3 md:grid-cols-2">
            <div className="space-y-1 md:col-span-2">
              <Label className="text-xs">Playbook title</Label>
              <Input
                className="border-line bg-ink"
                value={draft.title}
                onChange={(e) => setDraft((d) => ({ ...d, title: e.target.value }))}
                placeholder="e.g. Quarterly vendor review"
              />
            </div>
            <div className="space-y-1">
              <Label className="text-xs">Short tab label</Label>
              <Input
                className="border-line bg-ink"
                value={draft.tab_label}
                onChange={(e) => setDraft((d) => ({ ...d, tab_label: e.target.value }))}
                placeholder="Shown in the library tab"
              />
            </div>
            <div className="space-y-1">
              <Label className="text-xs">Team or department (optional)</Label>
              <Input
                className="border-line bg-ink"
                value={draft.department ?? ""}
                onChange={(e) =>
                  setDraft((d) => ({ ...d, department: e.target.value.trim() || undefined }))
                }
                placeholder="e.g. Operations"
              />
            </div>
            <div className="space-y-1 md:col-span-2">
              <Label className="text-xs">When do people use this?</Label>
              <Textarea
                className="min-h-[88px] border-line bg-ink text-sm"
                value={draft.summary}
                onChange={(e) => setDraft((d) => ({ ...d, summary: e.target.value }))}
                placeholder="Plain-language summary: trigger, audience, success criteria."
              />
            </div>
          </div>

          <Collapsible>
            <CollapsibleTrigger className="flex w-full items-center gap-1 rounded-md border border-line bg-black/25 px-3 py-2 text-left text-xs font-medium text-gray-300 hover:bg-black/40 [&[data-state=open]>svg]:rotate-180">
              <ChevronDown className="size-4 shrink-0 transition-transform duration-200" />
              Technical options (id, export filename)
            </CollapsibleTrigger>
            <CollapsibleContent className="mt-3 space-y-3 rounded-lg border border-line/80 bg-black/20 p-3">
              <div className="grid gap-3 md:grid-cols-2">
                <div className="space-y-1">
                  <Label className="text-xs text-gray-400">Stable id (for files &amp; API)</Label>
                  <Input
                    className="border-line bg-ink font-mono text-xs"
                    value={draft.id}
                    onChange={(e) => setDraft((d) => ({ ...d, id: e.target.value }))}
                    placeholder="letters, numbers, underscores"
                  />
                </div>
                <div className="space-y-1">
                  <Label className="text-xs text-gray-400">JSON download name</Label>
                  <Input
                    className="border-line bg-ink font-mono text-xs"
                    value={draft.jsonFilename}
                    onChange={(e) => setDraft((d) => ({ ...d, jsonFilename: e.target.value }))}
                  />
                </div>
              </div>
              <p className="text-[11px] text-gray-500">
                You can ignore these until you export or connect automations. The composer tidies empty lines and fills step
                ids from names when you save.
              </p>
            </CollapsibleContent>
          </Collapsible>

          <div className="flex justify-end gap-2">
            <Button type="button" size="sm" onClick={() => setWizardStep(1)}>
              Next: Steps
            </Button>
          </div>
        </div>
      ) : null}

      {wizardStep === 1 ? (
        <div className="space-y-4">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <p className="text-sm text-gray-400">
              Each step is a block of work. Add checklist items people can tick off; optional lines capture reminders for your
              systems (tickets, CRM, etc.).
            </p>
            <Button type="button" size="sm" variant="secondary" onClick={addPhase} className="shrink-0">
              Add step
            </Button>
          </div>

          <div className="space-y-3">
            {draft.phases.map((phase, i) => (
              <Card key={i} className="border-line bg-panel">
                <CardHeader className="pb-2">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <CardTitle className="text-sm text-white">Step {i + 1}</CardTitle>
                    <div className="flex flex-wrap gap-1">
                      <Button type="button" size="sm" variant="ghost" className="h-7 px-2 text-xs" onClick={() => movePhase(i, -1)}>
                        Move up
                      </Button>
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        className="h-7 px-2 text-xs"
                        onClick={() => movePhase(i, 1)}
                      >
                        Move down
                      </Button>
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        className="h-7 px-2 text-xs text-destructive"
                        onClick={() => removePhase(i)}
                      >
                        Remove
                      </Button>
                    </div>
                  </div>
                </CardHeader>
                <CardContent className="space-y-4">
                  <div className="grid gap-3 md:grid-cols-2">
                    <div className="space-y-1 md:col-span-2">
                      <Label className="text-xs">Step name</Label>
                      <Input
                        className="border-line bg-ink"
                        value={phase.name}
                        onChange={(e) => updatePhase(i, { name: e.target.value })}
                        placeholder="e.g. Collect invoices"
                      />
                    </div>
                    <div className="space-y-2 md:col-span-2">
                      <Label className="text-xs">Who leads this step?</Label>
                      <Select
                        value={phase.actor}
                        onValueChange={(v) => updatePhase(i, { actor: v as SopPhase["actor"] })}
                      >
                        <SelectTrigger className="border-line bg-ink">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="ai">Assistant leads</SelectItem>
                          <SelectItem value="human">People lead</SelectItem>
                          <SelectItem value="both">People and assistant together</SelectItem>
                        </SelectContent>
                      </Select>
                      <ActorHint actor={phase.actor} />
                    </div>
                  </div>

                  <div className="space-y-1">
                    <Label className="text-xs">What should happen (instructions)</Label>
                    <Textarea
                      className="min-h-[72px] border-line bg-ink text-sm"
                      value={phase.sop_logic}
                      onChange={(e) => updatePhase(i, { sop_logic: e.target.value })}
                      placeholder="Plain-language guidance, decisions, links, or policies for this step."
                    />
                  </div>

                  <ChecklistField
                    label="Checklist — concrete tasks"
                    hint="One row per task. These become actionable bullets in the playbook."
                    placeholder="e.g. Email finance for missing receipts"
                    lines={phase.actions}
                    onChange={(next) => updatePhase(i, { actions: next })}
                    addLabel="Add checklist item"
                  />

                  <ChecklistField
                    label="System hooks (optional)"
                    hint="Short reminders like “Open ticket in …” or “Update CRM deal stage.” Skip if you do not use Company OS language yet."
                    placeholder="e.g. Create procurement ticket"
                    lines={phase.company_os}
                    onChange={(next) => updatePhase(i, { company_os: next })}
                    addLabel="Add system reminder"
                  />

                  <Collapsible>
                    <CollapsibleTrigger className="flex w-full items-center gap-1 rounded-md border border-line bg-black/25 px-3 py-2 text-left text-xs font-medium text-gray-300 hover:bg-black/40 [&[data-state=open]>svg]:rotate-180">
                      <ChevronDown className="size-4 shrink-0 transition-transform duration-200" />
                      Technical options for this step
                    </CollapsibleTrigger>
                    <CollapsibleContent className="mt-3 grid gap-3 md:grid-cols-2">
                      <div className="space-y-1 md:col-span-2">
                        <Label className="text-xs text-gray-400">Step id (auto-filled from name if empty)</Label>
                        <Input
                          className="border-line bg-ink font-mono text-xs"
                          value={phase.id}
                          onChange={(e) => updatePhase(i, { id: e.target.value })}
                        />
                      </div>
                      <div className="space-y-1 md:col-span-2">
                        <Label className="text-xs text-gray-400">Resolution note (optional)</Label>
                        <Input
                          className="border-line bg-ink text-sm"
                          value={phase.resolution ?? ""}
                          onChange={(e) => updatePhase(i, { resolution: e.target.value || undefined })}
                        />
                      </div>
                      <div className="space-y-1 md:col-span-2">
                        <Label className="text-xs text-gray-400">Escalation (optional)</Label>
                        <Input
                          className="border-line bg-ink text-sm"
                          value={phase.escalation ?? ""}
                          onChange={(e) => updatePhase(i, { escalation: e.target.value || undefined })}
                        />
                      </div>
                    </CollapsibleContent>
                  </Collapsible>
                </CardContent>
              </Card>
            ))}
          </div>

          <div className="flex flex-wrap justify-between gap-2">
            <Button type="button" size="sm" variant="outline" className="border-line" onClick={() => setWizardStep(0)}>
              Back
            </Button>
            <Button type="button" size="sm" onClick={() => setWizardStep(2)}>
              Next: Audit trail
            </Button>
          </div>
        </div>
      ) : null}

      {wizardStep === 2 ? (
        <div className="space-y-4">
          <p className="text-sm text-gray-400">
            Choose whether this playbook should suggest standard log entries (helpful for leadership visibility). You can
            always leave this off and keep logging manual.
          </p>

          <div className="grid gap-3 sm:grid-cols-2">
            <button
              type="button"
              onClick={() => setGovernancePreset("recommended")}
              className={cn(
                "rounded-lg border p-4 text-left transition-colors",
                governanceMode === "recommended"
                  ? "border-primary/60 bg-primary/10 ring-1 ring-primary/30"
                  : "border-line bg-black/20 hover:border-line-strong"
              )}
            >
              <div className="flex items-center gap-2">
                <Badge variant="outline" className="border-emerald-500/40 text-emerald-200">
                  Suggested
                </Badge>
                <span className="text-sm font-semibold text-white">Standard audit suggestions</span>
              </div>
              <p className="mt-2 text-xs leading-relaxed text-gray-400">
                Adds three lightweight suggestions: step started, milestone completed, workflow finished. Matches how built-in
                examples behave.
              </p>
            </button>
            <button
              type="button"
              onClick={() => setGovernancePreset("off")}
              className={cn(
                "rounded-lg border p-4 text-left transition-colors",
                governanceMode === "off"
                  ? "border-primary/60 bg-primary/10 ring-1 ring-primary/30"
                  : "border-line bg-black/20 hover:border-line-strong"
              )}
            >
              <div className="flex items-center gap-2">
                <Badge variant="outline" className="border-gray-500/40 text-gray-300">
                  Manual
                </Badge>
                <span className="text-sm font-semibold text-white">No automatic suggestions</span>
              </div>
              <p className="mt-2 text-xs leading-relaxed text-gray-400">
                Best if you only want the checklist and instructions, or you will type log notes yourself when running the
                playbook.
              </p>
            </button>
          </div>

          {governanceMode === "custom" ? (
            <div className="rounded-md border border-amber-500/30 bg-amber-950/20 px-3 py-2 text-xs text-amber-100">
              This template uses <strong className="font-medium">custom</strong> audit entries (often from an import). Use
              Technical options below to edit them, or pick one of the presets above to replace them.
            </div>
          ) : null}

          {governanceMode !== "off" ? (
            <div className="space-y-1">
              <Label className="text-xs">Note for leadership (shown with the audit trail)</Label>
              <Textarea
                className="min-h-[72px] border-line bg-ink text-sm"
                value={draft.interaction_log.description}
                onChange={(e) =>
                  setDraft((d) => ({
                    ...d,
                    interaction_log: { ...d.interaction_log, description: e.target.value },
                  }))
                }
              />
            </div>
          ) : null}

          <Collapsible>
            <CollapsibleTrigger className="flex w-full items-center gap-1 rounded-md border border-line bg-black/25 px-3 py-2 text-left text-xs font-medium text-gray-300 hover:bg-black/40 [&[data-state=open]>svg]:rotate-180">
              <ChevronDown className="size-4 shrink-0 transition-transform duration-200" />
              Technical options — raw audit rows
            </CollapsibleTrigger>
            <CollapsibleContent className="mt-3 space-y-3">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-xs text-gray-500">Governance events</span>
                <Button type="button" size="sm" variant="secondary" onClick={addGov}>
                  Add row
                </Button>
              </div>
              {draft.interaction_log.suggested_events.map((ev, gi) => (
                <Card key={gi} className="border-line/80 bg-black/20">
                  <CardContent className="grid gap-2 pt-4 md:grid-cols-2">
                    <Input
                      className="border-line bg-ink font-mono text-xs"
                      placeholder="action"
                      value={ev.action}
                      onChange={(e) => updateGov(gi, { action: e.target.value })}
                    />
                    <div className="flex gap-2">
                      <Input
                        className="border-line bg-ink font-mono text-xs"
                        placeholder="subject_type"
                        value={ev.subject_type}
                        onChange={(e) => updateGov(gi, { subject_type: e.target.value })}
                      />
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        className="shrink-0 text-destructive"
                        onClick={() => removeGov(gi)}
                      >
                        Remove
                      </Button>
                    </div>
                    <Input
                      className="md:col-span-2 border-line bg-ink font-mono text-xs"
                      placeholder="subject_hint"
                      value={ev.subject_hint}
                      onChange={(e) => updateGov(gi, { subject_hint: e.target.value })}
                    />
                    <Input
                      className="md:col-span-2 border-line bg-ink font-mono text-xs"
                      placeholder="payload_summary"
                      value={ev.payload_summary}
                      onChange={(e) => updateGov(gi, { payload_summary: e.target.value })}
                    />
                  </CardContent>
                </Card>
              ))}
            </CollapsibleContent>
          </Collapsible>

          <div className="flex flex-wrap justify-between gap-2">
            <Button type="button" size="sm" variant="outline" className="border-line" onClick={() => setWizardStep(1)}>
              Back
            </Button>
            <Button type="button" size="sm" variant="secondary" onClick={() => setWizardStep(0)}>
              Review basics
            </Button>
          </div>
        </div>
      ) : null}

      {validation.length > 0 ? (
        <ul className="rounded border border-amber-900/50 bg-amber-950/30 px-3 py-2 text-xs text-amber-100">
          {validation.map((err) => (
            <li key={err}>{err}</li>
          ))}
        </ul>
      ) : null}

      <Separator className="bg-line" />

      <div className="flex flex-wrap gap-2">
        <Button type="button" size="sm" variant="secondary" onClick={() => setValidation(runValidate())}>
          Check for issues
        </Button>
        <Button type="button" size="sm" onClick={saveToWorkspace}>
          Save to this workspace (browser)
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="border-line"
          disabled={!loadSavedId}
          onClick={deleteSaved}
        >
          Delete saved
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="border-line"
          disabled={!normalizedDraft}
          onClick={() => normalizedDraft && downloadJson(normalizedDraft)}
        >
          Download JSON
        </Button>
        <Button
          type="button"
          size="sm"
          className="bg-primary text-primary-foreground"
          disabled={working || !normalizedDraft}
          onClick={implement}
        >
          {working ? "Implementing…" : "Implement in workspace"}
        </Button>
      </div>
    </div>
  );
}
