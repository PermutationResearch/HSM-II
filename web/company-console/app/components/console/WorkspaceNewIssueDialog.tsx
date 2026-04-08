"use client";

import { useEffect, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Paperclip } from "lucide-react";
import { Badge } from "@/app/components/ui/badge";
import { Button } from "@/app/components/ui/button";
import { Checkbox } from "@/app/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/app/components/ui/dialog";
import { Input } from "@/app/components/ui/input";
import { Label } from "@/app/components/ui/label";
import { Textarea } from "@/app/components/ui/textarea";
import { companyOsUrl } from "@/app/lib/company-api-url";
import { specificationWithWorkspacePaths, truncatePath } from "@/app/lib/workspace-issue";

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  apiBase: string;
  companyId: string;
  /** `hsmii_home`-relative paths attached to the task. */
  workspacePaths: string[];
  /** Shown in "For …" checkbox, e.g. Corey. */
  assigneeDisplayName: string;
  /** Stored as `owner_persona` when the checkbox is checked (usually `company_agents.name`). */
  assigneePersona: string;
};

export function WorkspaceNewIssueDialog({
  open,
  onOpenChange,
  apiBase,
  companyId,
  workspacePaths,
  assigneeDisplayName,
  assigneePersona,
}: Props) {
  const qc = useQueryClient();
  const [title, setTitle] = useState("");
  const [bodyExtra, setBodyExtra] = useState("");
  const [assignToAgent, setAssignToAgent] = useState(true);
  const [isPlanMode, setIsPlanMode] = useState(false);

  const pathKey = workspacePaths.join("\0");

  const createTask = useMutation({
    mutationFn: async () => {
      const t = title.trim();
      if (!t) throw new Error("Title is required.");
      const paths = workspacePaths.map((p) => p.trim()).filter(Boolean);
      const specification = specificationWithWorkspacePaths(bodyExtra, paths);
      const payload: Record<string, unknown> = {
        title: t,
        specification: specification || null,
        workspace_attachment_paths: paths.length ? paths : undefined,
      };
      if (assignToAgent && assigneePersona.trim()) {
        payload.owner_persona = assigneePersona.trim();
      }
      if (isPlanMode) {
        payload.capability_refs = [{ kind: "mode", ref: "plan" }];
      }
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/tasks`), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(payload),
      });
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    onSuccess: () => {
      setTitle("");
      setBodyExtra("");
      setAssignToAgent(true);
      setIsPlanMode(false);
      onOpenChange(false);
      void qc.invalidateQueries({ queryKey: ["hsm", "tasks", apiBase, companyId] });
      void qc.invalidateQueries({ queryKey: ["hsm", "intelligence", apiBase, companyId] });
    },
  });

  useEffect(() => {
    if (!open) return;
    setTitle("");
    setBodyExtra("");
    setAssignToAgent(true);
    setIsPlanMode(false);
  }, [open, pathKey]);

  const primaryPath = workspacePaths[0]?.trim() ?? "";
  const showBanner = workspacePaths.filter(Boolean).length > 0;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="top-[5%] max-h-[min(90vh,720px)] translate-y-0 overflow-y-auto sm:max-w-2xl md:top-[50%] md:max-h-[min(90vh,800px)] md:translate-y-[-50%]"
        showCloseButton
      >
        <DialogHeader>
          <DialogTitle>New issue</DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          {showBanner ? (
            <div
              className="flex gap-2 rounded-md border border-amber-500/45 bg-amber-500/10 px-3 py-2 text-sm text-amber-950 dark:border-amber-400/35 dark:bg-amber-400/10 dark:text-amber-50"
              role="status"
            >
              <Paperclip className="mt-0.5 h-4 w-4 shrink-0 opacity-80" aria-hidden />
              <div>
                <p className="font-medium">Workspace file attached</p>
                <p className="mt-0.5 font-mono text-xs opacity-90">
                  {workspacePaths.length === 1
                    ? truncatePath(primaryPath, 64)
                    : `${workspacePaths.length} paths · ${truncatePath(primaryPath, 48)}`}
                </p>
              </div>
            </div>
          ) : null}

          <div className="space-y-2">
            <Label htmlFor="ws-issue-title">Title</Label>
            <Input
              id="ws-issue-title"
              placeholder="Issue title"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
            />
          </div>

          <div className="flex flex-wrap items-center gap-3">
            <div className="flex items-center gap-2">
              <Checkbox
                id="ws-issue-assign"
                checked={assignToAgent}
                onCheckedChange={(v) => setAssignToAgent(v === true)}
              />
              <Label htmlFor="ws-issue-assign" className="text-sm font-normal cursor-pointer">
                For {assigneeDisplayName}
              </Label>
            </div>
          </div>

          <div className="flex flex-wrap gap-1.5">
            {(["Project", "Task", "Todo", "Priority", "Labels"] as const).map((tag) => (
              <Badge key={tag} variant="outline" className="font-normal text-muted-foreground" title="Not wired yet">
                {tag}
              </Badge>
            ))}
          </div>

          <div className="flex items-center gap-2">
            <Checkbox
              id="ws-issue-plan"
              checked={isPlanMode}
              onCheckedChange={(v) => setIsPlanMode(v === true)}
            />
            <Label htmlFor="ws-issue-plan" className="cursor-pointer text-sm font-normal">
              Plan mode — once approved, click Build on the issue to create an implementation issue
            </Label>
          </div>

          <div className="space-y-2">
            <Label htmlFor="ws-issue-body">Description</Label>
            <Textarea
              id="ws-issue-body"
              className="min-h-[160px] font-mono text-xs"
              placeholder="Add context above the workspace pointer…"
              value={bodyExtra}
              onChange={(e) => setBodyExtra(e.target.value)}
            />
            {primaryPath ? (
              <p className="font-mono text-[11px] text-muted-foreground">
                {specificationWithWorkspacePaths("", [primaryPath]).trim()}
              </p>
            ) : null}
          </div>

          {createTask.isError ? (
            <p className="text-sm text-destructive">
              {createTask.error instanceof Error ? createTask.error.message : String(createTask.error)}
            </p>
          ) : null}

          <p className="text-[11px] text-muted-foreground">
            Creates a Company OS task (<span className="font-mono">POST …/tasks</span>) with{" "}
            <span className="font-mono">workspace_attachment_paths</span> and the{" "}
            <span className="font-mono">Workspace file:</span> line merged into the specification.
            {isPlanMode ? (
              <>
                {" "}
                Adds <span className="font-mono">capability_refs=[{"{ kind: \"mode\", ref: \"plan\" }"}]</span>.
              </>
            ) : null}
          </p>
        </div>

        <DialogFooter className="gap-2 sm:justify-between">
          <Button type="button" variant="ghost" size="sm" onClick={() => onOpenChange(false)}>
            Discard
          </Button>
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-[11px] text-muted-foreground">Advanced, images: use Issues page</span>
            <Button
              type="button"
              className="bg-orange-600 text-white hover:bg-orange-600/90 dark:bg-orange-600 dark:hover:bg-orange-600/90"
              disabled={!title.trim() || createTask.isPending || workspacePaths.length === 0}
              onClick={() => createTask.mutate(undefined)}
            >
              {createTask.isPending ? "Creating…" : "Create issue"}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
