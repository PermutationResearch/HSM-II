/**
 * Claude Code–style single-turn harness for the agent chat stream.
 *
 * Maintains an ordered list of interleaved tool events and text segments
 * as they arrive from the NDJSON stream, so the UI can render tools and
 * assistant text in arrival order.
 *
 * Usage:
 *   const harness = new AgentChatTurnHarness();
 *   harness.reset();                        // new turn starts
 *   harness.beginFreshTextSegment();        // message_start → new text slot
 *   harness.appendTextDelta("Hello ");      // content_block_delta
 *   harness.appendTool(runtimeToolEvent);   // runtime tool_start / tool_result
 *   const items = harness.getItems();       // render
 */

/** Mirrors the `RuntimeToolEvent` shape from WorkspaceRightRail — kept loose so both sources work. */
export type HarnessToolEvent = {
  event_type?: string;
  task_key?: string | null;
  tool_name?: string | null;
  call_id?: string | null;
  success?: boolean;
  message?: string;
  input?: unknown;
  ts_ms?: number;
};

/** A single entry in the harness item list, in strict arrival order. */
export type HarnessTurnItem =
  | { kind: "tool"; seq: number; event: HarnessToolEvent }
  | { kind: "text"; seq: number; text: string };

export class AgentChatTurnHarness {
  private items: HarnessTurnItem[] = [];
  private seq = 0;

  /** Clear all items and reset the sequence counter. Call at the start of each new turn. */
  reset(): void {
    this.items = [];
    this.seq = 0;
  }

  /**
   * Start a fresh text segment. Subsequent `appendTextDelta` calls accumulate into this
   * new slot. Called when a `message_start` event resets the Anthropic stream.
   */
  beginFreshTextSegment(): void {
    this.items.push({ kind: "text", seq: this.seq++, text: "" });
  }

  /**
   * Append a text chunk to the latest text segment.
   * Creates one if there is no current text segment at the tail.
   */
  appendTextDelta(chunk: string): void {
    if (!chunk) return;
    const last = this.items[this.items.length - 1];
    if (last && last.kind === "text") {
      last.text += chunk;
    } else {
      this.items.push({ kind: "text", seq: this.seq++, text: chunk });
    }
  }

  /** Append a tool event. Creates a new tool item in sequence. */
  appendTool(event: HarnessToolEvent): void {
    this.items.push({ kind: "tool", seq: this.seq++, event });
  }

  /**
   * Return a snapshot of all items in arrival order.
   * Filters out empty text segments so the renderer never shows blank gaps.
   */
  getItems(): HarnessTurnItem[] {
    return this.items.filter((it) => it.kind === "tool" || it.text.length > 0);
  }
}
