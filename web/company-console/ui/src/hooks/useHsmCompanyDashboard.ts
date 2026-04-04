import { useCallback, useEffect, useState } from "react";

/** Run snapshot merged into list tasks (task_run_snapshots + checkout/release). */
export type HsmTaskRun = {
  status: string;
  tool_calls: number;
  log_tail: string;
  finished_at?: string | null;
  updated_at?: string | null;
};

export type HsmTask = {
  id: string;
  title: string;
  state: string;
  specification?: string | null;
  owner_persona?: string | null;
  due_at?: string | null;
  decision_mode?: string;
  priority?: number;
  checked_out_by?: string | null;
  /** Per-company sequential key (shown as {issue_key_prefix}-{n}). */
  display_number?: number | null;
  run?: HsmTaskRun | null;
};

/** Workforce registry row from Company OS (may include injection_surfaces from API). */
export type HsmCompanyAgent = {
  id: string;
  name: string;
  role: string;
  title?: string | null;
  status: string;
  budget_monthly_cents?: number | null;
};

export type HsmGovEvent = {
  id: string;
  actor: string;
  action: string;
  subject_type: string;
  subject_id: string;
  created_at: string;
  payload?: unknown;
};

export type HsmSpend = {
  company_id: string;
  total_usd: number;
  by_kind: { kind: string; amount_usd: number }[];
};

export type HsmDashboardData = {
  tasks: HsmTask[];
  governance: HsmGovEvent[];
  spend: HsmSpend | null;
  companyAgents: HsmCompanyAgent[];
  postgresOk: boolean;
};

export function useHsmCompanyDashboard(apiBase: string, companyId: string | null) {
  const [data, setData] = useState<HsmDashboardData | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!companyId) {
      setData(null);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const health = await fetch(`${apiBase}/api/company/health`).then((r) => r.json() as Promise<{ postgres_configured?: boolean; postgres_ok?: boolean }>);
      const postgresOk = !!(health.postgres_configured && health.postgres_ok);
      const [tRes, gRes, sRes, aRes] = await Promise.all([
        fetch(`${apiBase}/api/company/companies/${companyId}/tasks`),
        fetch(`${apiBase}/api/company/companies/${companyId}/governance/events`),
        fetch(`${apiBase}/api/company/companies/${companyId}/spend/summary`),
        fetch(`${apiBase}/api/company/companies/${companyId}/agents`),
      ]);
      if (!tRes.ok) throw new Error(`tasks ${tRes.status}`);
      if (!gRes.ok) throw new Error(`governance ${gRes.status}`);
      const tJson = (await tRes.json()) as { tasks?: HsmTask[] };
      const gJson = (await gRes.json()) as { events?: HsmGovEvent[] };
      let spend: HsmSpend | null = null;
      if (sRes.ok) spend = (await sRes.json()) as HsmSpend;
      let companyAgents: HsmCompanyAgent[] = [];
      if (aRes.ok) {
        const aJson = (await aRes.json()) as { agents?: HsmCompanyAgent[] };
        companyAgents = aJson.agents ?? [];
      }
      setData({
        tasks: tJson.tasks ?? [],
        governance: gJson.events ?? [],
        spend,
        companyAgents,
        postgresOk,
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setData(null);
    } finally {
      setLoading(false);
    }
  }, [apiBase, companyId]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return { data, loading, error, refresh };
}
