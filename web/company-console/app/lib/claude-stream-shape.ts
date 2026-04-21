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

/** A fully-assembled tool_use block from the Anthropic wire protocol. */
export type CompletedToolUse = {
  name: string;
  tool_use_id: string;
  input_json: string;
};

/**
 * Reassembles Anthropic `tool_use` content blocks from a streaming `stream_event` sequence.
 *
 * The Anthropic protocol emits tool calls across three event types:
 *   1. `content_block_start`  — `{ type: "tool_use", id, name, input: {} }` at some block index
 *   2. `content_block_delta`  — `{ type: "input_json_delta", partial_json: "..." }` (may repeat)
 *   3. `content_block_stop`   — signals the block at that index is complete
 *
 * `consume(event)` returns a `CompletedToolUse` when a tool_use block is finished, else `null`.
 */
export class AnthropicToolUseWireAssembler {
  private slots: Map<
    number,
    { name: string; id: string; jsonBuf: string }
  > = new Map();

  reset(): void {
    this.slots.clear();
  }

  consume(event: unknown): CompletedToolUse | null {
    const e = event as Record<string, unknown> | null;
    if (!e || typeof e !== "object") return null;
    const type = typeof e.type === "string" ? e.type : "";

    if (type === "content_block_start") {
      const idx = typeof e.index === "number" ? e.index : -1;
      const cb = e.content_block as Record<string, unknown> | undefined;
      if (cb && cb.type === "tool_use" && idx >= 0) {
        this.slots.set(idx, {
          name: typeof cb.name === "string" ? cb.name : "",
          id: typeof cb.id === "string" ? cb.id : "",
          jsonBuf: "",
        });
      }
      return null;
    }

    if (type === "content_block_delta") {
      const idx = typeof e.index === "number" ? e.index : -1;
      const delta = e.delta as Record<string, unknown> | undefined;
      const slot = this.slots.get(idx);
      if (slot && delta && delta.type === "input_json_delta") {
        const chunk = typeof delta.partial_json === "string" ? delta.partial_json : "";
        slot.jsonBuf += chunk;
      }
      return null;
    }

    if (type === "content_block_stop") {
      const idx = typeof e.index === "number" ? e.index : -1;
      const slot = this.slots.get(idx);
      if (slot && slot.name) {
        this.slots.delete(idx);
        return {
          name: slot.name,
          tool_use_id: slot.id,
          input_json: slot.jsonBuf,
        };
      }
      return null;
    }

    return null;
  }
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
