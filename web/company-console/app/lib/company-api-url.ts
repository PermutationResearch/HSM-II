/**
 * Build Company OS request URLs without double slashes.
 * When `apiBase` is empty, returns a root-relative path for the Next.js proxy.
 *
 * If `NEXT_PUBLIC_API_BASE` already ends with `/api/company` and `path` also starts with
 * `/api/company`, strips the duplicate segment so requests do not hit `/api/company/api/company/...` (404).
 */
export function companyOsUrl(apiBase: string, path: string): string {
  let normalized = path.startsWith("/") ? path : `/${path}`;
  let base = (apiBase ?? "").trim().replace(/\/+$/, "");
  if (!base) return normalized;
  const apiCompany = "/api/company";
  if (base.endsWith(apiCompany) && normalized.startsWith(`${apiCompany}/`)) {
    normalized = normalized.slice(apiCompany.length);
    if (!normalized.startsWith("/")) normalized = `/${normalized}`;
  }
  // `NEXT_PUBLIC_API_BASE=http://host:port/api` + `/api/company/...` would otherwise become `/api/api/company/...`
  if (base.endsWith("/api") && normalized.startsWith(`${apiCompany}/`)) {
    base = base.slice(0, -4).replace(/\/+$/, "");
  }
  return `${base}${normalized}`;
}
