import { api } from "./client";
import type { DashboardSummary } from "./types";

// Raw HSM-II response shapes
interface HsmHealthResponse { status: string; service: string }
interface HsmCompanyHealth { postgres_configured: boolean; postgres_ok: boolean }
interface HsmStats { home: string; trail_lines: number; memory_markdown_files: number; agents_enabled: number; tasks_in_progress: number; company_os: boolean }

export async function getHealth(): Promise<{ status: string }> {
  return api.get<HsmHealthResponse>("/api/health");
}

export async function getDashboardSummary(): Promise<DashboardSummary> {
  const stats = await api.get<HsmStats>("/api/console/stats");
  return {
    companyOsEnabled: stats.company_os,
    agentsEnabled: stats.agents_enabled,
    tasksInProgress: stats.tasks_in_progress,
    trailLines: stats.trail_lines,
    memoryFiles: stats.memory_markdown_files,
  };
}

export async function getCompanyHealth(): Promise<HsmCompanyHealth> {
  return api.get<HsmCompanyHealth>("/api/company/health");
}
