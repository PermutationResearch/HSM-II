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

/**
 * Dedicated POST proxy so workspace file delete is not dropped by optional catch-all routing (404).
 */
export async function POST(req: NextRequest, ctx: { params: Promise<{ companyId: string }> }) {
  const { companyId } = await ctx.params;
  const dest = `${UPSTREAM}/api/company/companies/${companyId}/workspace/file/delete${req.nextUrl.search}`;
  const headers = new Headers();
  req.headers.forEach((value, key) => {
    const k = key.toLowerCase();
    if (!HOP_BY_HOP.has(k) && k !== "host") {
      headers.set(key, value);
    }
  });
  const body = await req.arrayBuffer();
  const res = await fetch(dest, {
    method: "POST",
    headers,
    body: body.byteLength ? body : undefined,
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
