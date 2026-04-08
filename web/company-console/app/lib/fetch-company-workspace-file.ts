import { companyOsUrl } from "@/app/lib/company-api-url";

export type FetchWorkspaceFileResult =
  | { ok: true; content: string }
  | { ok: false; status: number; error: string };

/** GET file under company hsmii_home (relative path, e.g. visions.md). */
export async function fetchCompanyWorkspaceFile(
  apiBase: string,
  companyId: string,
  relPath: string,
): Promise<FetchWorkspaceFileResult> {
  const url = companyOsUrl(
    apiBase,
    `/api/company/companies/${companyId}/workspace/file?path=${encodeURIComponent(relPath)}`,
  );
  const r = await fetch(url);
  const j = (await r.json().catch(() => ({}))) as { error?: string; content?: string };
  if (!r.ok) {
    return { ok: false, status: r.status, error: typeof j.error === "string" ? j.error : r.statusText };
  }
  return { ok: true, content: typeof j.content === "string" ? j.content : "" };
}
