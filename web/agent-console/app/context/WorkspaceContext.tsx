"use client";

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { useQueryClient } from "@tanstack/react-query";
import { getApiBase, useCompanies, useCompanyHealth } from "@/app/lib/hsm-queries";
import type { HsmCompanyRow } from "@/app/lib/hsm-api-types";

export type PropertiesSelection =
  | { kind: "task"; id: string; title?: string }
  | { kind: "agent"; id: string; name?: string }
  | null;

type WorkspaceContextValue = {
  apiBase: string;
  companyId: string | null;
  setCompanyId: (id: string | null) => void;
  companies: HsmCompanyRow[];
  companiesLoading: boolean;
  companiesError: Error | null;
  postgresOk: boolean;
  refreshWorkspace: () => Promise<void>;
  propertiesSelection: PropertiesSelection;
  setPropertiesSelection: (s: PropertiesSelection) => void;
};

const WorkspaceContext = createContext<WorkspaceContextValue | null>(null);

export function WorkspaceProvider({ children }: { children: ReactNode }) {
  const apiBase = useMemo(() => getApiBase(), []);
  const qc = useQueryClient();
  const { data: health } = useCompanyHealth(apiBase);
  const {
    data: companies = [],
    isLoading: companiesLoading,
    error: companiesError,
  } = useCompanies(apiBase);

  const [companyId, setCompanyId] = useState<string | null>(null);
  const [propertiesSelection, setPropertiesSelection] = useState<PropertiesSelection>(null);

  useEffect(() => {
    if (companies.length === 0) return;
    setCompanyId((prev) => {
      if (prev && companies.some((c) => c.id === prev)) return prev;
      return companies[0]?.id ?? null;
    });
  }, [companies]);

  const refreshWorkspace = useCallback(async () => {
    await qc.invalidateQueries({ queryKey: ["hsm"] });
  }, [qc]);

  const postgresOk = !!(health?.postgres_configured && health?.postgres_ok);

  const value = useMemo(
    (): WorkspaceContextValue => ({
      apiBase,
      companyId,
      setCompanyId,
      companies,
      companiesLoading,
      companiesError: companiesError instanceof Error ? companiesError : companiesError ? new Error(String(companiesError)) : null,
      postgresOk,
      refreshWorkspace,
      propertiesSelection,
      setPropertiesSelection,
    }),
    [
      apiBase,
      companyId,
      companies,
      companiesLoading,
      companiesError,
      postgresOk,
      refreshWorkspace,
      propertiesSelection,
    ],
  );

  return <WorkspaceContext.Provider value={value}>{children}</WorkspaceContext.Provider>;
}

export function useWorkspace() {
  const ctx = useContext(WorkspaceContext);
  if (!ctx) throw new Error("useWorkspace must be used within WorkspaceProvider");
  return ctx;
}
