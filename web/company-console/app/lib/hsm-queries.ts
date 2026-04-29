import { useQuery } from "@tanstack/react-query";
import { getConsoleApiBase } from "./console-api-base";
import { companyOsUrl } from "./company-api-url";
import { asArray, asObject } from "./runtime-contract";
import type {
  HsmAgentInventory,
  HsmCompanyAgentRow,
  HsmCompanyCredential,
  HsmBrowserProviderStatus,
  HsmMemoryArtifact,
  HsmMemoryInspect,
  HsmCompanyRow,
  HsmCompanyOpsOverview,
  HsmCompanyConnector,
  HsmCompanyProfile,
  HsmConnectorTemplate,
  HsmEmailOperatorQueueItem,
  HsmGoalRow,
  HsmIntelligenceSummary,
  HsmSelfImprovementProposal,
  HsmSelfImprovementSummary,
  HsmStorePromotion,
  HsmIssueLabelRow,
  HsmProjectRow,
  HsmSkillBankEntry,
  HsmThreadSession,
  HsmWorkflowPack,
  HsmOperatorInbox,
  HsmSpendSummaryRow,
  HsmTaskRow,
  HsmMissionControlSummary,
} from "./hsm-api-types";

export function getApiBase(): string {
  return getConsoleApiBase();
}

export type HsmCompanyHealth = {
  postgres_configured?: boolean;
  postgres_ok?: boolean;
};

async function readJsonObject(r: Response): Promise<Record<string, unknown>> {
  const raw = await r.json().catch(() => ({}));
  return asObject(raw) ?? {};
}

function getErrorMessage(obj: Record<string, unknown>, fallback: string): string {
  return typeof obj.error === "string" && obj.error.trim() ? obj.error : fallback;
}

export function useCompanyHealth(apiBase: string) {
  const url =
    apiBase.length > 0
      ? `${apiBase}/api/company/health`
      : "/api/company/health";
  return useQuery({
    queryKey: ["hsm", "health", apiBase],
    queryFn: async () => {
      const r = await fetch(url);
      const body = await readJsonObject(r);
      if (!r.ok) {
        throw new Error(`health ${r.status}`);
      }
      return {
        postgres_configured:
          typeof body.postgres_configured === "boolean" ? body.postgres_configured : undefined,
        postgres_ok: typeof body.postgres_ok === "boolean" ? body.postgres_ok : undefined,
      } satisfies HsmCompanyHealth;
    },
  });
}

export function useCompanies(apiBase: string) {
  return useQuery({
    queryKey: ["hsm", "companies", apiBase],
    queryFn: async () => {
      const r = await fetch(`${apiBase}/api/company/companies`);
      const j = await readJsonObject(r);
      if (!r.ok) {
        throw new Error(getErrorMessage(j, `companies ${r.status}`));
      }
      return asArray(j.companies) as HsmCompanyRow[];
    },
  });
}

export function useCompanyTasks(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "tasks", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/tasks`);
      const j = await readJsonObject(r);
      if (!r.ok) {
        throw new Error(getErrorMessage(j, `tasks ${r.status}`));
      }
      return asArray(j.tasks) as HsmTaskRow[];
    },
    enabled: !!companyId,
  });
}

export function useTaskQueue(apiBase: string, companyId: string | null, view?: string) {
  return useQuery({
    queryKey: ["hsm", "task-queue", apiBase, companyId, view ?? "all"],
    queryFn: async () => {
      const qs = new URLSearchParams();
      if (view?.trim()) qs.set("view", view.trim());
      const r = await fetch(
        companyOsUrl(apiBase, `/api/company/companies/${companyId}/tasks/queue?${qs.toString()}`),
      );
      const j = await readJsonObject(r);
      if (!r.ok) throw new Error(getErrorMessage(j, `task queue ${r.status}`));
      return asArray(j.tasks) as HsmTaskRow[];
    },
    enabled: !!companyId,
    refetchInterval: 15_000,
  });
}

export function useCompanyAgents(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "agents", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/agents`);
      const j = await readJsonObject(r);
      if (!r.ok) {
        throw new Error(getErrorMessage(j, `agents ${r.status}`));
      }
      return asArray(j.agents) as HsmCompanyAgentRow[];
    },
    enabled: !!companyId,
  });
}

export function useAgentInventory(apiBase: string, companyId: string | null, agentId: string | null) {
  return useQuery({
    queryKey: ["hsm", "agent-inventory", apiBase, companyId, agentId],
    queryFn: async () => {
      if (!companyId || !agentId) throw new Error("missing company or agent");
      const url = companyOsUrl(
        apiBase,
        `/api/company/companies/${companyId}/agents/${agentId}/inventory`,
      );
      const r = await fetch(url);
      const text = await r.text();
      let j: Record<string, unknown> = {};
      try {
        j = asObject(text ? JSON.parse(text) : {}) ?? {};
      } catch {
        j = {};
      }
      if (!r.ok) {
        const errBody = typeof j.error === "string" ? j.error.trim() : "";
        if (r.status === 404 && !errBody) {
          throw new Error(
            "Inventory API missing on this server (empty 404). Rebuild and restart hsm_console from the repo: `cargo run -p hyper-stigmergy --bin hsm_console -- --port 3847` so GET …/agents/:agentId/inventory exists. Ensure HSM_CONSOLE_URL (Next proxy) and NEXT_PUBLIC_API_BASE point at that process.",
          );
        }
        if (errBody) throw new Error(errBody);
        throw new Error(`inventory ${r.status}`);
      }
      return j as HsmAgentInventory;
    },
    enabled: !!companyId && !!agentId,
  });
}

export function useCompanySpendSummary(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "spend", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/spend/summary`);
      const j = await readJsonObject(r);
      if (!r.ok) {
        throw new Error(getErrorMessage(j, `spend ${r.status}`));
      }
      return j as HsmSpendSummaryRow;
    },
    enabled: !!companyId,
  });
}

export function useCompanyGoals(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "goals", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/goals`);
      const j = await readJsonObject(r);
      if (!r.ok) {
        throw new Error(getErrorMessage(j, `goals ${r.status}`));
      }
      return asArray(j.goals) as HsmGoalRow[];
    },
    enabled: !!companyId,
  });
}

export function useCompanyProjects(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "projects", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/projects`));
      const j = await readJsonObject(r);
      if (r.status === 404) {
        return [];
      }
      if (!r.ok) {
        throw new Error(getErrorMessage(j, `projects ${r.status}`));
      }
      return asArray(j.projects) as HsmProjectRow[];
    },
    enabled: !!companyId,
    retry: false,
  });
}

export function useCompanyIssueLabels(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "issue-labels", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/issue-labels`));
      const j = await readJsonObject(r);
      if (r.status === 404) {
        return [];
      }
      if (!r.ok) {
        throw new Error(getErrorMessage(j, `issue-labels ${r.status}`));
      }
      return asArray(j.labels) as HsmIssueLabelRow[];
    },
    enabled: !!companyId,
    retry: false,
  });
}

/** GET …/agents/:id/operator-thread — stigmergic operator notes + compact digest */
export type HsmOperatorThreadResponse = {
  agent_id: string;
  agent_name: string;
  total_tasks: number;
  compact_digest: string;
  tasks?: unknown[];
  notes_flat?: { task_id?: string; task_title?: string; note?: unknown }[];
};

export function useAgentOperatorThread(apiBase: string, companyId: string | null, agentId: string | null) {
  return useQuery({
    queryKey: ["hsm", "operator-thread", apiBase, companyId, agentId],
    queryFn: async () => {
      if (!companyId || !agentId) throw new Error("missing company or agent");
      const r = await fetch(
        companyOsUrl(apiBase, `/api/company/companies/${companyId}/agents/${agentId}/operator-thread`),
      );
      const j = await readJsonObject(r);
      if (!r.ok) {
        throw new Error(getErrorMessage(j, `operator-thread ${r.status}`));
      }
      return j as HsmOperatorThreadResponse;
    },
    enabled: !!companyId && !!agentId,
  });
}

export function useCompanyIntelligenceSummary(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "intelligence", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/intelligence/summary`);
      const j = await readJsonObject(r);
      if (!r.ok) {
        throw new Error(getErrorMessage(j, `intelligence ${r.status}`));
      }
      return j as HsmIntelligenceSummary;
    },
    enabled: !!companyId,
    refetchInterval: 15_000,
  });
}

export function useSelfImprovementSummary(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "self-improvement", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/self-improvement/summary`);
      const j = await readJsonObject(r);
      if (!r.ok) {
        throw new Error(getErrorMessage(j, `self-improvement ${r.status}`));
      }
      const summary = asObject(j.summary) as HsmSelfImprovementSummary | null;
      return (
        summary ?? {
          total_failures_7d: 0,
          repeat_failure_rate_7d: 0,
          first_pass_success_rate_7d: 1,
          proposals_created_7d: 0,
          proposals_applied_7d: 0,
          rollback_rate_7d: 0,
          avg_recovery_hours_7d: null,
        }
      );
    },
    enabled: !!companyId,
    refetchInterval: 30_000,
  });
}

export function useSelfImprovementProposals(
  apiBase: string,
  companyId: string | null,
  status?: string,
) {
  return useQuery({
    queryKey: ["hsm", "self-improvement-proposals", apiBase, companyId, status ?? ""],
    queryFn: async () => {
      const qs = new URLSearchParams();
      qs.set("limit", "120");
      if (status?.trim()) qs.set("status", status.trim());
      const r = await fetch(
        `${apiBase}/api/company/companies/${companyId}/self-improvement/proposals?${qs.toString()}`,
      );
      const j = await readJsonObject(r);
      if (!r.ok) {
        throw new Error(getErrorMessage(j, `self-improvement proposals ${r.status}`));
      }
      return asArray(j.proposals) as HsmSelfImprovementProposal[];
    },
    enabled: !!companyId,
    refetchInterval: 30_000,
  });
}

export function useStorePromotions(
  apiBase: string,
  companyId: string | null,
  sourceStore?: string,
) {
  return useQuery({
    queryKey: ["hsm", "store-promotions", apiBase, companyId, sourceStore ?? ""],
    queryFn: async () => {
      const qs = new URLSearchParams();
      qs.set("limit", "200");
      if (sourceStore?.trim()) qs.set("source_store", sourceStore.trim());
      const r = await fetch(
        `${apiBase}/api/company/companies/${companyId}/promotions?${qs.toString()}`,
      );
      const j = await readJsonObject(r);
      if (!r.ok) {
        throw new Error(getErrorMessage(j, `promotions ${r.status}`));
      }
      return asArray(j.promotions) as HsmStorePromotion[];
    },
    enabled: !!companyId,
    refetchInterval: 30_000,
  });
}

export function useMemoryArtifacts(
  apiBase: string,
  companyId: string | null,
  status?: string,
) {
  return useQuery({
    queryKey: ["hsm", "memory-artifacts", apiBase, companyId, status ?? ""],
    queryFn: async () => {
      const qs = new URLSearchParams();
      qs.set("limit", "80");
      if (status?.trim()) qs.set("status", status.trim());
      const r = await fetch(
        `${apiBase}/api/company/companies/${companyId}/memory/artifacts?${qs.toString()}`,
      );
      const j = await readJsonObject(r);
      if (!r.ok) {
        throw new Error(getErrorMessage(j, `memory artifacts ${r.status}`));
      }
      return asArray(j.artifacts) as HsmMemoryArtifact[];
    },
    enabled: !!companyId,
    refetchInterval: 15_000,
  });
}

export function useMemoryInspect(
  apiBase: string,
  companyId: string | null,
  memoryId: string | null,
) {
  return useQuery({
    queryKey: ["hsm", "memory-inspect", apiBase, companyId, memoryId],
    queryFn: async () => {
      if (!companyId || !memoryId) throw new Error("missing company or memory");
      const r = await fetch(
        `${apiBase}/api/company/companies/${companyId}/memory/${memoryId}/inspect`,
      );
      const j = await readJsonObject(r);
      if (!r.ok) {
        throw new Error(getErrorMessage(j, `memory inspect ${r.status}`));
      }
      return j as HsmMemoryInspect;
    },
    enabled: !!companyId && !!memoryId,
  });
}

export function useCompanyCredentials(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "company-credentials", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/credentials`));
      const j = await readJsonObject(r);
      // Older backend builds may not expose credentials endpoints yet.
      if (r.status === 404) {
        return [];
      }
      if (!r.ok) {
        throw new Error(getErrorMessage(j, `credentials ${r.status}`));
      }
      return asArray(j.credentials) as HsmCompanyCredential[];
    },
    enabled: !!companyId,
  });
}

export function useSkillBank(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "skill-bank", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(
        `${apiBase}/api/company/companies/${companyId}/skills/bank?include_body=0&max_body_bytes=0`,
      );
      const j = await readJsonObject(r);
      if (!r.ok) {
        throw new Error(getErrorMessage(j, `skill-bank ${r.status}`));
      }
      return {
        current_skills: asArray(j.current_skills) as HsmSkillBankEntry[],
        recommended_skills: asArray(j.recommended_skills) as HsmSkillBankEntry[],
        connected_skill_refs: (asObject(j.connected_skill_refs) as Record<string, string[]>) ?? {},
        active_agent_count: typeof j.active_agent_count === "number" ? j.active_agent_count : 0,
      };
    },
    enabled: !!companyId,
  });
}

export async function fetchSkillBankEntry(
  apiBase: string,
  companyId: string,
  slug: string,
): Promise<HsmSkillBankEntry> {
  const q = new URLSearchParams({ slug });
  const r = await fetch(
    `${apiBase}/api/company/companies/${companyId}/skills/bank/entry?${q.toString()}`,
  );
  const j = await readJsonObject(r);
  if (!r.ok) {
    throw new Error(getErrorMessage(j, `skill-bank-entry ${r.status}`));
  }
  return asObject(j.skill) as HsmSkillBankEntry;
}

export function useBrowserProviders(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "browser-providers", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(
        companyOsUrl(apiBase, `/api/company/companies/${companyId}/browser/providers`),
      );
      const j = await readJsonObject(r);
      if (r.status === 404) return [];
      if (!r.ok) throw new Error(getErrorMessage(j, `browser providers ${r.status}`));
      return asArray(j.providers) as HsmBrowserProviderStatus[];
    },
    enabled: !!companyId,
  });
}

export function useThreadSessions(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "thread-sessions", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/thread-sessions`);
      const j = await readJsonObject(r);
      if (!r.ok) throw new Error(getErrorMessage(j, `thread sessions ${r.status}`));
      return asArray(j.sessions) as HsmThreadSession[];
    },
    enabled: !!companyId,
  });
}

export function useCompanyOpsOverview(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "ops-overview", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/ops/overview`));
      const j = await readJsonObject(r);
      if (!r.ok) throw new Error(getErrorMessage(j, `ops overview ${r.status}`));
      return j as HsmCompanyOpsOverview;
    },
    enabled: !!companyId,
    refetchInterval: 30_000,
  });
}

export function useCompanyConnectors(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "connectors", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/connectors`));
      const j = await readJsonObject(r);
      if (!r.ok) throw new Error(getErrorMessage(j, `connectors ${r.status}`));
      return asArray(j.connectors) as HsmCompanyConnector[];
    },
    enabled: !!companyId,
  });
}

export function useConnectorTemplates(apiBase: string, category?: string, companyId?: string | null) {
  return useQuery({
    queryKey: ["hsm", "connector-templates", apiBase, category ?? "all", companyId ?? "none"],
    queryFn: async () => {
      const qs = new URLSearchParams();
      if (category?.trim()) qs.set("category", category.trim());
      if (companyId?.trim()) qs.set("company_id", companyId.trim());
      const r = await fetch(companyOsUrl(apiBase, `/api/company/connectors/templates?${qs.toString()}`));
      const j = await readJsonObject(r);
      if (!r.ok) throw new Error(getErrorMessage(j, `connector templates ${r.status}`));
      return asArray(j.templates) as HsmConnectorTemplate[];
    },
  });
}

export function useEmailOperatorQueue(
  apiBase: string,
  companyId: string | null,
  status: string = "pending_approval",
) {
  return useQuery({
    queryKey: ["hsm", "email-operator-queue", apiBase, companyId, status],
    queryFn: async () => {
      const qs = new URLSearchParams();
      qs.set("status", status);
      const r = await fetch(
        companyOsUrl(apiBase, `/api/company/companies/${companyId}/email/operator-queue?${qs.toString()}`),
      );
      const j = await readJsonObject(r);
      if (!r.ok) throw new Error(getErrorMessage(j, `email queue ${r.status}`));
      return asArray(j.items) as HsmEmailOperatorQueueItem[];
    },
    enabled: !!companyId,
    refetchInterval: 10_000,
  });
}

export function useCompanyProfile(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "company-profile", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/profile`));
      const j = await readJsonObject(r);
      if (!r.ok) throw new Error(getErrorMessage(j, `profile ${r.status}`));
      return (asObject(j.profile) as HsmCompanyProfile | null) ?? null;
    },
    enabled: !!companyId,
  });
}

export function useWorkflowPacks(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "workflow-packs", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/workflow-packs`));
      const j = await readJsonObject(r);
      if (!r.ok) throw new Error(getErrorMessage(j, `workflow packs ${r.status}`));
      return asArray(j.workflow_packs) as HsmWorkflowPack[];
    },
    enabled: !!companyId,
  });
}

export function useOperatorInbox(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "operator-inbox", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/operator-inbox`));
      const j = await readJsonObject(r);
      if (!r.ok) throw new Error(getErrorMessage(j, `operator inbox ${r.status}`));
      return j as HsmOperatorInbox;
    },
    enabled: !!companyId,
    refetchInterval: 10_000,
  });
}

export function useMissionControl(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "mission-control", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/mission-control`));
      const j = await readJsonObject(r);
      if (!r.ok) throw new Error(getErrorMessage(j, `mission-control ${r.status}`));
      return j as HsmMissionControlSummary;
    },
    enabled: !!companyId,
    refetchInterval: 10_000,
  });
}
