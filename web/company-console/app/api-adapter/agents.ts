import { api } from "./client";
import type { Agent, OrgNode } from "./types";

interface HsmAgentRow {
  id: string; company_id: string; name: string; role: string; title?: string | null;
  capabilities?: string | null; reports_to?: string | null; adapter_type?: string | null;
  adapter_config?: unknown; budget_monthly_cents?: number | null; briefing?: string | null;
  status: string; sort_order: number; created_at?: string; updated_at?: string;
}

function mapAgent(raw: HsmAgentRow): Agent {
  return {
    id: raw.id, companyId: raw.company_id, name: raw.name, role: raw.role,
    title: raw.title, status: raw.status, capabilities: raw.capabilities,
    reportsTo: raw.reports_to, adapterType: raw.adapter_type,
    adapterConfig: raw.adapter_config, budgetMonthlyCents: raw.budget_monthly_cents,
    briefing: raw.briefing, sortOrder: raw.sort_order,
    createdAt: raw.created_at, updatedAt: raw.updated_at,
  };
}

export async function listAgents(companyId: string): Promise<Agent[]> {
  const data = await api.get<{ agents: HsmAgentRow[] }>(`/api/company/companies/${companyId}/agents`);
  return data.agents.map(mapAgent);
}

export async function getOrg(companyId: string): Promise<{ agents: Agent[]; tree: OrgNode[] }> {
  const data = await api.get<{ agents: HsmAgentRow[]; tree: unknown[] }>(`/api/company/companies/${companyId}/org`);
  return { agents: data.agents.map(mapAgent), tree: data.tree as OrgNode[] };
}

export async function createAgent(companyId: string, input: Partial<Agent>): Promise<Agent> {
  const data = await api.post<HsmAgentRow>(`/api/company/companies/${companyId}/agents`, {
    name: input.name, role: input.role, title: input.title,
    capabilities: input.capabilities, reports_to: input.reportsTo,
    adapter_type: input.adapterType, adapter_config: input.adapterConfig,
    budget_monthly_cents: input.budgetMonthlyCents, briefing: input.briefing,
    sort_order: input.sortOrder,
  });
  return mapAgent(data);
}

export async function updateAgent(companyId: string, agentId: string, patch: Partial<Agent>): Promise<Agent> {
  const data = await api.patch<HsmAgentRow>(`/api/company/companies/${companyId}/agents/${agentId}`, {
    name: patch.name, role: patch.role, title: patch.title,
    capabilities: patch.capabilities, reports_to: patch.reportsTo,
    adapter_type: patch.adapterType, adapter_config: patch.adapterConfig,
    budget_monthly_cents: patch.budgetMonthlyCents, briefing: patch.briefing,
  });
  return mapAgent(data);
}
