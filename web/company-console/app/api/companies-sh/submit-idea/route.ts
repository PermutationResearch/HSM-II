import fs from "fs";
import path from "path";
import { NextResponse } from "next/server";

export const runtime = "nodejs";

type Body = {
  title?: string;
  summary?: string;
  contact?: string;
  link?: string;
};

/**
 * When HSM_COMPANY_PACK_SUBMISSIONS_DIR is set, appends one JSON line per idea for operators to review.
 * Otherwise returns fallback_url (Paperclip contributing guide).
 */
export async function POST(req: Request) {
  try {
    const body = (await req.json()) as Body;
    const title = typeof body.title === "string" ? body.title.trim() : "";
    const summary = typeof body.summary === "string" ? body.summary.trim() : "";
    if (!title || !summary) {
      return NextResponse.json({ error: "Title and summary are required." }, { status: 400 });
    }
    const contact = typeof body.contact === "string" ? body.contact.trim() : "";
    const link = typeof body.link === "string" ? body.link.trim() : "";

    const dir = process.env.HSM_COMPANY_PACK_SUBMISSIONS_DIR?.trim();
    if (!dir) {
      return NextResponse.json({
        accepted: false,
        message:
          "This console is not configured to store submissions on disk. Use the upstream guide to propose packs for the public directory.",
        fallback_url: "https://github.com/paperclipai/companies/blob/main/CONTRIBUTING.md",
      });
    }

    const resolved = path.resolve(dir);
    await fs.promises.mkdir(resolved, { recursive: true });
    const file = path.join(resolved, "company-pack-ideas.jsonl");
    const line =
      JSON.stringify({
        at: new Date().toISOString(),
        title,
        summary,
        contact: contact || undefined,
        link: link || undefined,
      }) + "\n";
    await fs.promises.appendFile(file, line, "utf8");

    return NextResponse.json({ accepted: true, stored: file });
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    return NextResponse.json({ error: msg }, { status: 500 });
  }
}
