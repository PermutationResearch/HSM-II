/**
 * POST streaming chat against HSM-II Axum (`/api/llm/chat/stream` via Next rewrite `/hsm-api/...`).
 * Parses SSE `data: {...}` lines (Axum sse::Event).
 */

const DEFAULT_BASE = "/hsm-api";

export type StreamChatMessage = { role: string; content: string };

export type StreamEvent =
  | { type: "delta"; text: string }
  | { type: "done"; model: string; usage?: unknown; provider?: string }
  | { type: "error"; message: string };

function parseSseDataLines(buffer: string, onEvent: (ev: StreamEvent) => void): string {
  let rest = buffer;
  while (true) {
    const nl = rest.indexOf("\n");
    if (nl === -1) break;
    const line = rest.slice(0, nl).replace(/\r$/, "");
    rest = rest.slice(nl + 1);
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith(":")) continue;
    if (trimmed.startsWith("data:")) {
      const raw = trimmed.slice(5).trim();
      try {
        const j = JSON.parse(raw) as Record<string, unknown>;
        const t = j.type;
        if (t === "delta" && typeof j.text === "string") {
          onEvent({ type: "delta", text: j.text });
        } else if (t === "done") {
          onEvent({
            type: "done",
            model: typeof j.model === "string" ? j.model : "",
            usage: j.usage,
            provider: typeof j.provider === "string" ? j.provider : undefined,
          });
        } else if (t === "error") {
          onEvent({
            type: "error",
            message: typeof j.message === "string" ? j.message : "unknown error",
          });
        }
      } catch {
        /* ignore malformed chunk */
      }
    }
  }
  return rest;
}

export async function streamLlmChat(
  messages: StreamChatMessage[],
  opts?: {
    basePath?: string;
    model?: string;
    temperature?: number;
    onDelta?: (text: string) => void;
    onDone?: (ev: Extract<StreamEvent, { type: "done" }>) => void;
    onError?: (message: string) => void;
  }
): Promise<string> {
  const base = opts?.basePath ?? DEFAULT_BASE;
  const url = `${base}/llm/chat/stream`;
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: "text/event-stream" },
    body: JSON.stringify({
      messages,
      model: opts?.model,
      temperature: opts?.temperature ?? 0.7,
    }),
  });

  if (!res.ok) {
    const t = await res.text().catch(() => "");
    throw new Error(`${res.status} ${res.statusText}${t ? ` — ${t.slice(0, 200)}` : ""}`);
  }

  const reader = res.body?.getReader();
  if (!reader) throw new Error("No response body");

  const dec = new TextDecoder();
  let carry = "";
  let full = "";

  const dispatch = (ev: StreamEvent) => {
    if (ev.type === "delta") {
      full += ev.text;
      opts?.onDelta?.(ev.text);
    } else if (ev.type === "done") {
      opts?.onDone?.(ev);
    } else if (ev.type === "error") {
      opts?.onError?.(ev.message);
    }
  };

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    carry += dec.decode(value, { stream: true });
    carry = parseSseDataLines(carry, dispatch);
  }
  carry += dec.decode();
  carry = parseSseDataLines(carry, dispatch);

  return full;
}
