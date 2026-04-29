import { asObject } from "@/app/lib/runtime-contract";

/**
 * Normalize assistant `message.content` from OpenAI-compatible responses
 * (plain string or multimodal / provider-specific part arrays).
 */
export function normalizeChatCompletionMessageContent(content: unknown): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  let out = "";
  for (const part of content) {
    const p = asObject(part);
    if (!p) continue;
    if (typeof p.text === "string") out += p.text;
    else if (typeof p.output_text === "string") out += p.output_text;
  }
  return out;
}

/**
 * Incremental text from `choices[0].delta` in OpenRouter / OpenAI-compatible SSE chunks.
 */
export function openRouterStreamDeltaToText(delta: unknown): string {
  const d = asObject(delta);
  if (!d) return "";
  let s = normalizeChatCompletionMessageContent(d.content);
  for (const key of ["reasoning", "reasoning_content", "thinking"] as const) {
    const v = d[key];
    if (typeof v === "string" && v.length > 0) s += v;
  }
  return s;
}

/**
 * Best-effort extraction of assistant text from non-stream chat completion payloads.
 * Handles OpenRouter/OpenAI-compatible and Ollama-like response shapes.
 */
export function extractReplyFromChatCompletionPayload(payload: unknown): string {
  const root = asObject(payload);
  if (!root) return "";

  const fromMsgObj = (msg: unknown): string => {
    const m = asObject(msg);
    if (!m) return "";
    let out = "";
    out += normalizeChatCompletionMessageContent(m.content);
    if (typeof m.output_text === "string") out += m.output_text;
    // Some providers place final text in reasoning-like fields on free/local models.
    if (!out.trim()) {
      for (const key of ["reasoning", "reasoning_content", "thinking"] as const) {
        const v = m[key];
        if (typeof v === "string" && v.length > 0) out += v;
      }
    }
    return out;
  };

  const choices = Array.isArray(root.choices) ? root.choices : [];
  for (const choice of choices) {
    const c = asObject(choice);
    if (!c) continue;
    const msgText = fromMsgObj(c.message);
    if (msgText.trim()) return msgText;
    if (typeof c.text === "string" && c.text.trim()) return c.text;
  }

  const topMsgText = fromMsgObj(root.message);
  if (topMsgText.trim()) return topMsgText;
  if (typeof root.response === "string" && root.response.trim()) return root.response;
  return "";
}
