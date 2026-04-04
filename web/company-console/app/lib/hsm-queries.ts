import { useQuery } from "@tanstack/react-query";
import { getConsoleApiBase } from "./console-api-base";
import type { HsmCompanyAgentRow, HsmCompanyRow, HsmSpendSummaryRow, HsmTaskRow } from "./hsm-api-types";

export function getApiBase(): string {
  return getConsoleApiBase();
}

export function useCompanyHealth(apiBase: string) {
  return useQuery({
    queryKey: ["hsm", "health", apiBase],
    queryFn: async () => {
      const r = await fetch(`${apiBase}/api/company/health`);
      return (await r.json()) as {
        postgres_configured?: boolean;
        postgres_ok?: boolean;
      };
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
