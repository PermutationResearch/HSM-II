import type { NextRequest } from "next/server";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const UPSTREAM = (process.env.HSM_CONSOLE_URL ?? "http://127.0.0.1:3847").replace(/\/+$/, "");

const HOP_BY_HOP = new Set([
  "connection",
  "keep-alive",
  "proxy-authenticate",
  "proxy-authorization",
  "te",
  "trailers",
  "transfer-encoding",
  "upgrade",
]);

type Body = { companyId?: unknown; path?: unknown };

/**
 * Flat POST proxy for workspace file delete. Static segment wins over `[[...path]]`, avoiding
 * Next.js 404s on deep routes like `.../companies/[id]/workspace/file/delete` in some setups.
 */
export async function POST(req: NextRequest) {
  let parsed: Body;
  try {
    parsed = (await req.json()) as Body;
  } catch {
    return Response.json({ error: "invalid JSON body" }, { status: 400 });
  }
  const companyId = typeof parsed.companyId === "string" ? parsed.companyId.trim() : "";
  const relPath = typeof parsed.path === "string" ? parsed.path.trim() : "";
  if (!companyId || !relPath) {
    return Response.json({ error: "companyId and path are required" }, { status: 400 });
  }

  const dest = `${UPSTREAM}/api/company/companies/${companyId}/workspace/file/delete${req.nextUrl.search}`;
  const headers = new Headers();
  req.headers.forEach((value, key) => {
    const k = key.toLowerCase();
    if (
      !HOP_BY_HOP.has(k) &&
      k !== "host" &&
      k !== "content-length" &&
      k !== "content-type"
    ) {
      headers.set(key, value);
    }
  });
  headers.set("Content-Type", "application/json");

  const res = await fetch(dest, {
    method: "POST",
    headers,
    body: JSON.stringify({ path: relPath }),
    redirect: "manual",
  });
  const out = new Headers(res.headers);
  out.delete("transfer-encoding");
  return new Response(res.body, {
    status: res.status,
    statusText: res.statusText,
    headers: out,
  });
}
