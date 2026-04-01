import { NextResponse } from "next/server";

/** Proxy [companies.sh](https://companies.sh/) open directory JSON (avoids browser CORS). */
export const revalidate = 3600;

export async function GET() {
  try {
    const r = await fetch("https://companies.sh/api/companies", {
      headers: { Accept: "application/json" },
      next: { revalidate: 3600 },
    });
    if (!r.ok) {
      return NextResponse.json(
        { error: `companies.sh returned ${r.status}`, items: [] },
        { status: 502 }
      );
    }
    const data = (await r.json()) as unknown;
    return NextResponse.json(data);
  } catch (e) {
    const msg = e instanceof Error ? e.message : "fetch failed";
    return NextResponse.json({ error: msg, items: [] }, { status: 502 });
  }
}
