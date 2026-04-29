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

/** Completed `tool_use` block after `content_block_stop` (indices ≥ 1 are reserved for runtime tools). */
export type AnthropicCompletedToolUseWire = {
  block_index: number;
  tool_use_id: string;
  name: string;
  input_json: string;
};

/**
 * Reassembles Anthropic-style `tool_use` streaming (start → `input_json_delta`* → `stop`).
 * Deltas are **concatenated** in order (`partial_json` fragments), matching Anthropic’s wire
 * and HSM’s cumulative `tool_start_delta.input.partial_json` previews (suffixes are re-spliced
 * server-side before emit).
 */
export class AnthropicToolUseWireAssembler {
  private byIndex = new Map<number, { id: string; name: string; json: string }>();

  reset(): void {
    this.byIndex.clear();
  }

  /**
   * Feed one inner `event` from `{ type: "stream_event", event }`.
   * Clears tool buffer on `message_start`. Returns a completed tool when `content_block_stop`
   * closes a `tool_use` block we were tracking.
   */
  consume(event: unknown): AnthropicCompletedToolUseWire | null {
    const o = event as Record<string, unknown> | null;
    if (!o || typeof o !== "object") return null;
    const typ = typeof o.type === "string" ? o.type : "";
    if (typ === "message_start") {
      this.reset();
      return null;
    }
    const idxRaw = o.index;
    const idx = typeof idxRaw === "number" ? idxRaw : Number(idxRaw);
    if (!Number.isFinite(idx)) return null;

    if (typ === "content_block_start") {
      const cb = o.content_block as Record<string, unknown> | undefined;
      if (cb && cb.type === "tool_use") {
        const id = typeof cb.id === "string" ? cb.id : "";
        const name = typeof cb.name === "string" ? cb.name : "";
        this.byIndex.set(idx, { id, name, json: "" });
      }
      return null;
    }

    if (typ === "content_block_delta") {
      const d = o.delta as Record<string, unknown> | undefined;
      if (!d || d.type !== "input_json_delta" || typeof d.partial_json !== "string") return null;
      const cur = this.byIndex.get(idx);
      if (!cur) return null;
      cur.json += d.partial_json;
      return null;
    }

    if (typ === "content_block_stop") {
      const cur = this.byIndex.get(idx);
      this.byIndex.delete(idx);
      if (!cur || !cur.name) return null;
      return {
        block_index: idx,
        tool_use_id: cur.id,
        name: cur.name,
        input_json: cur.json,
      };
    }

    return null;
  }
}

/** One-shot `tool_use` block (empty `input` in start, full JSON via one `input_json_delta`, then stop). */
export function anthropicContentBlockStartToolUse(
  index: number,
  toolUseId: string,
  name: string,
): AnthropicStreamEvent {
  return {
    type: "content_block_start",
    index,
    content_block: {
      type: "tool_use",
      id: toolUseId,
      name,
      input: {},
    },
  };
}

export function anthropicToolUseInputJsonDelta(index: number, partialJson: string): AnthropicStreamEvent {
  return {
    type: "content_block_delta",
    index,
    delta: { type: "input_json_delta", partial_json: partialJson },
  };
}

export function anthropicContentBlockStopIndex(index: number): AnthropicStreamEvent {
  return {
    type: "content_block_stop",
    index,
  };
}
