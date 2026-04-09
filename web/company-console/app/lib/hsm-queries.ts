import { useQuery } from "@tanstack/react-query";
import { getConsoleApiBase } from "./console-api-base";
import { companyOsUrl } from "./company-api-url";
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
} from "./hsm-api-types";

export function getApiBase(): string {
  return getConsoleApiBase();
}

export type HsmCompanyHealth = {
  postgres_configured?: boolean;
  postgres_ok?: boolean;
};

export function useCompanyHealth(apiBase: string) {
  const url =
    apiBase.length > 0
      ? `${apiBase}/api/company/health`
      : "/api/company/health";
  return useQuery({
    queryKey: ["hsm", "health", apiBase],
    queryFn: async () => {
      const r = await fetch(url);
      const body = (await r.json().catch(() => ({}))) as HsmCompanyHealth;
      if (!r.ok) {
        throw new Error(`health ${r.status}`);
      }
      return body;
    },
  });
}

export function useCompanies(apiBase: string) {
  return useQuery({
    queryKey: ["hsm", "companies", apiBase],
    queryFn: async () => {
      const r = await fetch(`${apiBase}/api/company/companies`);
      const j = (await r.json().catch(() => ({}))) as {
        companies?: HsmCompanyRow[];
        error?: string;
      };
      if (!r.ok) {
        throw new Error(j.error ?? `companies ${r.status}`);
      }
      return j.companies ?? [];
    },
  });
}

export function useCompanyTasks(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "tasks", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/tasks`);
      const j = (await r.json().catch(() => ({}))) as { tasks?: HsmTaskRow[]; error?: string };
      if (!r.ok) {
        throw new Error(j.error ?? `tasks ${r.status}`);
      }
      return j.tasks ?? [];
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
      const j = (await r.json().catch(() => ({}))) as { tasks?: HsmTaskRow[]; error?: string };
      if (!r.ok) throw new Error(j.error ?? `task queue ${r.status}`);
      return j.tasks ?? [];
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
      const j = (await r.json().catch(() => ({}))) as {
        agents?: HsmCompanyAgentRow[];
        error?: string;
      };
      if (!r.ok) {
        throw new Error(j.error ?? `agents ${r.status}`);
      }
      return j.agents ?? [];
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
      type InvErr = { error?: string };
      let j: (HsmAgentInventory & InvErr) | InvErr = {};
      try {
        j = text ? (JSON.parse(text) as (HsmAgentInventory & InvErr) | InvErr) : {};
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
      const j = (await r.json().catch(() => ({}))) as HsmSpendSummaryRow & { error?: string };
      if (!r.ok) {
        throw new Error(j.error ?? `spend ${r.status}`);
      }
      return j;
    },
    enabled: !!companyId,
  });
}

export function useCompanyGoals(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "goals", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/goals`);
      const j = (await r.json().catch(() => ({}))) as { goals?: HsmGoalRow[]; error?: string };
      if (!r.ok) {
        throw new Error(j.error ?? `goals ${r.status}`);
      }
      return j.goals ?? [];
    },
    enabled: !!companyId,
  });
}

export function useCompanyProjects(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "projects", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/projects`));
      const j = (await r.json().catch(() => ({}))) as { projects?: HsmProjectRow[]; error?: string };
      if (r.status === 404) {
        return [];
      }
      if (!r.ok) {
        throw new Error(j.error ?? `projects ${r.status}`);
      }
      return j.projects ?? [];
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
      const j = (await r.json().catch(() => ({}))) as { labels?: HsmIssueLabelRow[]; error?: string };
      if (r.status === 404) {
        return [];
      }
      if (!r.ok) {
        throw new Error(j.error ?? `issue-labels ${r.status}`);
      }
      return j.labels ?? [];
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
      const j = (await r.json().catch(() => ({}))) as HsmOperatorThreadResponse & { error?: string };
      if (!r.ok) {
        throw new Error(j.error ?? `operator-thread ${r.status}`);
      }
      return j;
    },
    enabled: !!companyId && !!agentId,
  });
}

export function useCompanyIntelligenceSummary(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "intelligence", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/intelligence/summary`);
      const j = (await r.json().catch(() => ({}))) as HsmIntelligenceSummary;
      if (!r.ok) {
        throw new Error(j.error ?? `intelligence ${r.status}`);
      }
      return j;
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
      const j = (await r.json().catch(() => ({}))) as { summary?: HsmSelfImprovementSummary; error?: string };
      if (!r.ok) {
        throw new Error(j.error ?? `self-improvement ${r.status}`);
      }
      return (
        j.summary ?? {
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
      const j = (await r.json().catch(() => ({}))) as {
        proposals?: HsmSelfImprovementProposal[];
        error?: string;
      };
      if (!r.ok) {
        throw new Error(j.error ?? `self-improvement proposals ${r.status}`);
      }
      return j.proposals ?? [];
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
      const j = (await r.json().catch(() => ({}))) as {
        promotions?: HsmStorePromotion[];
        error?: string;
      };
      if (!r.ok) {
        throw new Error(j.error ?? `promotions ${r.status}`);
      }
      return j.promotions ?? [];
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
      const j = (await r.json().catch(() => ({}))) as {
        artifacts?: HsmMemoryArtifact[];
        error?: string;
      };
      if (!r.ok) {
        throw new Error(j.error ?? `memory artifacts ${r.status}`);
      }
      return j.artifacts ?? [];
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
      const j = (await r.json().catch(() => ({}))) as HsmMemoryInspect & { error?: string };
      if (!r.ok) {
        throw new Error(j.error ?? `memory inspect ${r.status}`);
      }
      return j;
    },
    enabled: !!companyId && !!memoryId,
  });
}

export function useCompanyCredentials(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "company-credentials", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/credentials`));
      const j = (await r.json().catch(() => ({}))) as {
        credentials?: HsmCompanyCredential[];
        error?: string;
      };
      // Older backend builds may not expose credentials endpoints yet.
      if (r.status === 404) {
        return [];
      }
      if (!r.ok) {
        throw new Error(j.error ?? `credentials ${r.status}`);
      }
      return j.credentials ?? [];
    },
    enabled: !!companyId,
  });
}

export function useSkillBank(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "skill-bank", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/skills/bank`);
      const j = (await r.json().catch(() => ({}))) as {
        current_skills?: HsmSkillBankEntry[];
        recommended_skills?: HsmSkillBankEntry[];
        connected_skill_refs?: Record<string, string[]>;
        active_agent_count?: number;
        error?: string;
      };
      if (!r.ok) {
        throw new Error(j.error ?? `skill-bank ${r.status}`);
      }
      return {
        current_skills: j.current_skills ?? [],
        recommended_skills: j.recommended_skills ?? [],
        connected_skill_refs: j.connected_skill_refs ?? {},
        active_agent_count: j.active_agent_count ?? 0,
      };
    },
    enabled: !!companyId,
  });
}

export function useBrowserProviders(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "browser-providers", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(
        companyOsUrl(apiBase, `/api/company/companies/${companyId}/browser/providers`),
      );
      const j = (await r.json().catch(() => ({}))) as {
        providers?: HsmBrowserProviderStatus[];
        error?: string;
      };
      if (r.status === 404) return [];
      if (!r.ok) throw new Error(j.error ?? `browser providers ${r.status}`);
      return j.providers ?? [];
    },
    enabled: !!companyId,
  });
}

export function useThreadSessions(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "thread-sessions", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(`${apiBase}/api/company/companies/${companyId}/thread-sessions`);
      const j = (await r.json().catch(() => ({}))) as {
        sessions?: HsmThreadSession[];
        error?: string;
      };
      if (!r.ok) throw new Error(j.error ?? `thread sessions ${r.status}`);
      return j.sessions ?? [];
    },
    enabled: !!companyId,
  });
}

export function useCompanyOpsOverview(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "ops-overview", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/ops/overview`));
      const j = (await r.json().catch(() => ({}))) as HsmCompanyOpsOverview & { error?: string };
      if (!r.ok) throw new Error(j.error ?? `ops overview ${r.status}`);
      return j;
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
      const j = (await r.json().catch(() => ({}))) as { connectors?: HsmCompanyConnector[]; error?: string };
      if (!r.ok) throw new Error(j.error ?? `connectors ${r.status}`);
      return j.connectors ?? [];
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
      const j = (await r.json().catch(() => ({}))) as { templates?: HsmConnectorTemplate[]; error?: string };
      if (!r.ok) throw new Error(j.error ?? `connector templates ${r.status}`);
      return j.templates ?? [];
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
      const j = (await r.json().catch(() => ({}))) as { items?: HsmEmailOperatorQueueItem[]; error?: string };
      if (!r.ok) throw new Error(j.error ?? `email queue ${r.status}`);
      return j.items ?? [];
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
      const j = (await r.json().catch(() => ({}))) as { profile?: HsmCompanyProfile; error?: string };
      if (!r.ok) throw new Error(j.error ?? `profile ${r.status}`);
      return j.profile ?? null;
    },
    enabled: !!companyId,
  });
}

export function useWorkflowPacks(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "workflow-packs", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/workflow-packs`));
      const j = (await r.json().catch(() => ({}))) as { workflow_packs?: HsmWorkflowPack[]; error?: string };
      if (!r.ok) throw new Error(j.error ?? `workflow packs ${r.status}`);
      return j.workflow_packs ?? [];
    },
    enabled: !!companyId,
  });
}

export function useOperatorInbox(apiBase: string, companyId: string | null) {
  return useQuery({
    queryKey: ["hsm", "operator-inbox", apiBase, companyId],
    queryFn: async () => {
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/operator-inbox`));
      const j = (await r.json().catch(() => ({}))) as HsmOperatorInbox & { error?: string };
      if (!r.ok) throw new Error(j.error ?? `operator inbox ${r.status}`);
      return j;
    },
    enabled: !!companyId,
    refetchInterval: 10_000,
  });
}
