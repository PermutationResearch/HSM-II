/**
 * Base URL for `hsm_console` (no trailing slash, no `/api/company` suffix).
 *
 * - **Default (unset / empty):** `""` → browser uses same origin; App Route handlers proxy
 *   `/api/company/*` to `HSM_CONSOLE_URL` (default `http://127.0.0.1:3847`).
 * - **Override:** set `NEXT_PUBLIC_API_BASE` to the API **origin** only, e.g. `http://127.0.0.1:3847`.
 *   If the value ends with `/api/company`, that suffix is stripped so callers can keep using
 *   `${apiBase}/api/company/...` without doubling the path (which would 404).
 *   A trailing `/api` alone is also stripped (avoids `/api/api/company/...` when joining paths).
 */
export function getConsoleApiBase(): string {
  const raw = process.env.NEXT_PUBLIC_API_BASE;
  let v = (typeof raw === "string" ? raw : "").trim().replace(/\/+$/, "");
  const suffixCompany = "/api/company";
  if (v.endsWith(suffixCompany)) {
    v = v.slice(0, -suffixCompany.length).replace(/\/+$/, "");
  }
  // Common mistake: origin + `/api` then `${base}/api/company/...` → `/api/api/company/...` (404).
  if (v.endsWith("/api")) {
    v = v.slice(0, -4).replace(/\/+$/, "");
  }
  if (v.length > 0) return v;
  return "";
}
