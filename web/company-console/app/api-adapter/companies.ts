import { api } from "./client";
import type { Company } from "./types";

interface HsmCompanyRow {
  id: string; slug: string; display_name: string; hsmii_home?: string | null; issue_key_prefix?: string; created_at: string;
}

function mapCompany(raw: HsmCompanyRow): Company {
  return {
    id: raw.id,
    slug: raw.slug,
    name: raw.display_name,
    status: "active",
    createdAt: raw.created_at,
    hsmiiHome: raw.hsmii_home,
    issueKeyPrefix: raw.issue_key_prefix,
  };
}

export async function listCompanies(): Promise<Company[]> {
  const data = await api.get<{ companies: HsmCompanyRow[] }>("/api/company/companies");
  return data.companies.map(mapCompany);
}

export async function createCompany(input: { slug: string; name: string; hsmiiHome?: string }): Promise<Company> {
  const data = await api.post<{ company: HsmCompanyRow }>("/api/company/companies", {
    slug: input.slug,
    display_name: input.name,
    hsmii_home: input.hsmiiHome,
  });
  return mapCompany(data.company);
}

export async function exportCompany(companyId: string): Promise<unknown> {
  return api.get(`/api/company/companies/${companyId}/export`);
}

export async function importCompany(data: unknown): Promise<unknown> {
  return api.post("/api/company/import", data);
}
