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

async function proxyToRust(req: NextRequest, pathSegments: string[]): Promise<Response> {
  const sub = pathSegments.length ? pathSegments.join("/") : "";
  const dest = `${UPSTREAM}/api/console/${sub}${req.nextUrl.search}`;
  const headers = new Headers();
  req.headers.forEach((value, key) => {
    const k = key.toLowerCase();
    if (!HOP_BY_HOP.has(k) && k !== "host") {
      headers.set(key, value);
    }
  });
  const init: RequestInit = {
    method: req.method,
    headers,
    redirect: "manual",
  };
  if (req.method !== "GET" && req.method !== "HEAD") {
    init.body = await req.arrayBuffer();
  }
  const res = await fetch(dest, init);
  const out = new Headers(res.headers);
  out.delete("transfer-encoding");
  return new Response(res.body, {
    status: res.status,
    statusText: res.statusText,
    headers: out,
  });
}

type Ctx = { params: Promise<{ path?: string[] }> };

async function handle(req: NextRequest, ctx: Ctx) {
  const { path } = await ctx.params;
  return proxyToRust(req, path ?? []);
}

export const GET = handle;
export const HEAD = handle;
export const POST = handle;
export const PUT = handle;
export const PATCH = handle;
export const DELETE = handle;
