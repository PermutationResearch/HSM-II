/**
 * Base URL for `hsm_console` (no trailing slash).
 *
 * - **Default (unset / empty):** `""` → browser uses same origin; `next.config.mjs` rewrites
 *   `/api/company/*` and `/api/console/*` to `HSM_CONSOLE_URL` (default `http://127.0.0.1:3847`).
 * - **Override:** set `NEXT_PUBLIC_API_BASE` to call Rust directly (e.g. remote host).
 */
export function getConsoleApiBase(): string {
  const raw = process.env.NEXT_PUBLIC_API_BASE;
  const v = (typeof raw === "string" ? raw : "").trim().replace(/\/+$/, "");
  if (v.length > 0) return v;
  return "";
}
