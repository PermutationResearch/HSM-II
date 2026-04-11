/**
 * POST /api/agent-chat-reply/stream
 *
 * NDJSON stream (one JSON object per line) for operator chat:
 * - `runtime` / `runtime_raw`: tool/runtime events (mirrors Company OS SSE while worker runs)
 * - `stream_event`: Claude-style partial assistant stream (`event` = Anthropic stream object)
 * - `delta`: optional legacy token chunks (not emitted on OpenRouter path when `stream_event` is used)
 * - `phase`, `done`, `error`
 */
import { NextRequest } from "next/server";

import { runAgentChatNdjsonStream, type AgentChatRequestBody } from "@/app/lib/agent-chat-stream-server";
import { asObject } from "@/app/lib/runtime-contract";

function parseBody(raw: unknown): AgentChatRequestBody | null {
  const bodyObj = asObject(raw);
  if (!bodyObj) return null;
  const taskId = typeof bodyObj.taskId === "string" ? bodyObj.taskId : "";
  const persona = typeof bodyObj.persona === "string" ? bodyObj.persona : "";
  if (!taskId || !persona) return null;
  return {
    taskId,
    persona,
    companyId: typeof bodyObj.companyId === "string" ? bodyObj.companyId : undefined,
    title: typeof bodyObj.title === "string" ? bodyObj.title : undefined,
    role: typeof bodyObj.role === "string" ? bodyObj.role : undefined,
    notes: Array.isArray(bodyObj.notes) ? (bodyObj.notes as AgentChatRequestBody["notes"]) : [],
  };
}

export async function POST(req: NextRequest) {
  const bodyRaw = await req.json().catch(() => null);
  const body = parseBody(bodyRaw);
  if (!body) {
    return new Response(JSON.stringify({ type: "error", message: "taskId and persona required" }) + "\n", {
      status: 400,
      headers: { "Content-Type": "application/x-ndjson; charset=utf-8" },
    });
  }

  const encoder = new TextEncoder();
  const { readable, writable } = new TransformStream<Uint8Array, Uint8Array>();
  const streamWriter = writable.getWriter();

  const writeLine = async (obj: Record<string, unknown>) => {
    await streamWriter.write(encoder.encode(`${JSON.stringify(obj)}\n`));
  };

  void (async () => {
    try {
      await runAgentChatNdjsonStream(body, writeLine);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      try {
        await writeLine({ type: "error", message });
      } catch {
        /* ignore */
      }
    } finally {
      try {
        await streamWriter.close();
      } catch {
        /* ignore */
      }
    }
  })();

  return new Response(readable, {
    headers: {
      "Content-Type": "application/x-ndjson; charset=utf-8",
      "Cache-Control": "no-store",
      Connection: "keep-alive",
    },
  });
}
