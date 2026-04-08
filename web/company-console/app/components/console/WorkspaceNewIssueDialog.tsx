"use client";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/app/components/ui/dialog";
import { WorkspaceNewIssueForm } from "@/app/components/console/WorkspaceNewIssueForm";

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  apiBase: string;
  companyId: string;
  /** Pre-seed paths (e.g. currently open file). You can add more paths in the form. */
  workspacePaths?: string[];
  /** Shown on the assignee control, e.g. Corey. */
  assigneeDisplayName: string;
  /** Stored as `owner_persona` when assign-on is on (usually `company_agents.name`). */
  assigneePersona: string;
  /** Breadcrumb prefix before “> New issue” (e.g. company or COM). */
  breadcrumbLabel?: string;
};

export function WorkspaceNewIssueDialog({
  open,
  onOpenChange,
  apiBase,
  companyId,
  workspacePaths = [],
  assigneeDisplayName,
  assigneePersona,
  breadcrumbLabel = "COM",
}: Props) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="top-[5%] max-h-[min(90vh,720px)] translate-y-0 overflow-y-auto sm:max-w-2xl md:top-[50%] md:max-h-[min(90vh,860px)] md:translate-y-[-50%]"
        showCloseButton
      >
        <DialogHeader className="space-y-1 text-left">
          <p className="font-mono text-[11px] text-muted-foreground">
            <span className="text-foreground/80">{breadcrumbLabel}</span>
            <span className="mx-1.5 text-muted-foreground/80">&gt;</span>
            <span>New issue</span>
          </p>
          <DialogTitle className="text-xl tracking-tight">New issue</DialogTitle>
          <DialogDescription className="text-left text-xs leading-relaxed text-muted-foreground">
            Start with <strong className="text-foreground/90">Plan</strong> vs <strong className="text-foreground/90">Task</strong>
            , attach files from the workspace if you want, then set project, priority, labels, reviewers, and optional
            repeat cadence. Everything you pick is saved on the task.
          </DialogDescription>
        </DialogHeader>

        {open ? (
          <WorkspaceNewIssueForm
            key={companyId}
            apiBase={apiBase}
            companyId={companyId}
            workspacePaths={workspacePaths}
            assigneeDisplayName={assigneeDisplayName}
            assigneePersona={assigneePersona}
            idPrefix="ws-issue"
            showAttachBanner={false}
            onCloseRequest={() => onOpenChange(false)}
            onCreated={() => onOpenChange(false)}
          />
        ) : null}
      </DialogContent>
    </Dialog>
  );
}
