import { useQuery } from "@tanstack/react-query";
import { getConsoleApiBase } from "./console-api-base";
import { companyOsUrl } from "./company-api-url";
import type {
  HsmAgentInventory,
  HsmCompanyAgentRow,
  HsmCompanyRow,
  HsmGoalRow,
  HsmIntelligenceSummary,
  HsmIssueLabelRow,
  HsmProjectRow,
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
