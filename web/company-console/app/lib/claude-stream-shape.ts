/**
 * Anthropic Messages API–shaped stream events, matching Claude Code's
 * `SDKPartialAssistantMessage` (`type: "stream_event", event: …`) wire format
 * so agent-chat can reuse Claude-style consumers.
 *
 * @see external/claude-code-from-npm/package/unpacked/src/entrypoints/sdk/coreSchemas.ts
 */

import { randomUUID } from "crypto";

const TEXT_BLOCK_INDEX = 0;

export type AnthropicStreamEvent = Record<string, unknown>;

/** message_start — reset UI accumulation for a new assistant message */
export function anthropicMessageStart(model: string): AnthropicStreamEvent {
  return {
    type: "message_start",
    message: {
      id: `msg_${randomUUID().replace(/-/g, "")}`,
      type: "message",
      role: "assistant",
      model,
      content: [],
      stop_reason: null,
      stop_sequence: null,
      usage: { input_tokens: 0, output_tokens: 0 },
    },
  };
}

export function anthropicContentBlockStartText(): AnthropicStreamEvent {
  return {
    type: "content_block_start",
    index: TEXT_BLOCK_INDEX,
    content_block: { type: "text", text: "" },
  };
}

export function anthropicTextDelta(text: string): AnthropicStreamEvent {
  return {
    type: "content_block_delta",
    index: TEXT_BLOCK_INDEX,
    delta: { type: "text_delta", text },
  };
}

export function anthropicContentBlockStop(): AnthropicStreamEvent {
  return {
    type: "content_block_stop",
    index: TEXT_BLOCK_INDEX,
  };
}

export function anthropicMessageDelta(): AnthropicStreamEvent {
  return {
    type: "message_delta",
    delta: { stop_reason: null, stop_sequence: null },
  };
}

export function anthropicMessageStop(): AnthropicStreamEvent {
  return { type: "message_stop" };
}

export function wrapSdkStreamEvent(
  event: AnthropicStreamEvent,
  sessionId?: string,
): Record<string, unknown> {
  return {
    type: "stream_event",
    event,
    session_id: sessionId ?? "hsm-agent-chat",
    uuid: randomUUID(),
    parent_tool_use_id: null,
  };
}

/** Parse streamed assistant text from an Anthropic stream `event` object. */
export function extractAnthropicStreamTextEffect(
  event: unknown,
): "reset" | { append: string } | null {
  const o = event as Record<string, unknown> | null;
  if (!o || typeof o !== "object") return null;
  const typ = typeof o.type === "string" ? o.type : "";
  if (typ === "message_start") return "reset";
  if (typ === "content_block_delta") {
    const d = o.delta as Record<string, unknown> | undefined;
    if (d && d.type === "text_delta" && typeof d.text === "string") {
      return { append: d.text };
    }
  }
  return null;
}
