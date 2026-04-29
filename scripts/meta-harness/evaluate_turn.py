"""
evaluate_turn.py — Run one operator turn through the Company OS agent-chat harness
and return a scored result dict.

Usage:
    python evaluate_turn.py --company-id <id> --persona cto \
        --prompt "run repo-intel on this codebase" \
        --out /tmp/turn_001.ndjson
"""
import argparse
import asyncio
import json
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional, List

import httpx

NEXT_BASE = "http://localhost:3050"
HSM_BASE  = "http://localhost:3847"

# ── Company OS task creation ─────────────────────────────────────────────────

async def create_task(client: httpx.AsyncClient, company_id: str, persona: str, title: str) -> str:
    """Create a fresh task and return its ID."""
    r = await client.post(
        f"{HSM_BASE}/api/company/companies/{company_id}/tasks",
        json={"title": title, "owner_persona": persona},
        timeout=15,
    )
    r.raise_for_status()
    body = r.json()
    # API returns {"task": {...}} wrapper
    task = body.get("task") or body
    return task["id"]

# ── NDJSON stream ─────────────────────────────────────────────────────────────

async def stream_turn(
    client: httpx.AsyncClient,
    task_id: str,
    persona: str,
    company_id: str,
    prompt: str,
    extra_notes: Optional[List[dict]] = None,
) -> tuple:
    """
    POST to /api/agent-chat-reply/stream, collect all NDJSON lines, return (lines, elapsed_s).
    """
    notes = (extra_notes or []) + [
        {"actor": "operator", "text": prompt, "at": datetime.now(timezone.utc).isoformat()}
    ]
    body = {
        "taskId": task_id,
        "persona": persona,
        "companyId": company_id,
        "notes": notes,
    }

    lines: list[dict] = []
    t0 = time.monotonic()

    async with client.stream(
        "POST",
        f"{NEXT_BASE}/api/agent-chat-reply/stream",
        json=body,
        # Server worker telemetry can wait up to 300s (HSM_OPERATOR_CHAT_TELEMETRY_WAIT_EXEC_MS cap); stay above that.
        timeout=360,
    ) as resp:
        resp.raise_for_status()
        async for raw in resp.aiter_lines():
            raw = raw.strip()
            if not raw:
                continue
            try:
                obj = json.loads(raw)
            except json.JSONDecodeError:
                obj = {"type": "raw", "text": raw}
            lines.append(obj)

    elapsed = time.monotonic() - t0
    return lines, elapsed

# ── Scoring ───────────────────────────────────────────────────────────────────

def score_turn(lines: list[dict], elapsed_s: float) -> dict:
    """
    Derive harness quality metrics from the NDJSON stream.

    Returns a dict with:
        finalized        bool   — finalize_response tool fired
        final_answer_len int    — char length of the final answer text
        tool_calls       int    — total tool invocations observed
        loop_iters       int    — inferred loop iteration count
        latency_s        float  — wall-clock seconds
        error            bool   — any error line present
        phase_sequence   list   — ordered list of phase names seen
        score            float  — composite 0-1 (higher = better)
    """
    finalized = False
    final_answer_len = 0
    tool_calls = 0
    loop_iters = 0
    error = False
    phase_sequence: list[str] = []
    streamed_text = ""

    for line in lines:
        t = line.get("type", "")

        if t == "error":
            error = True

        elif t == "phase":
            ph = line.get("phase", "")
            if ph:
                phase_sequence.append(ph)

        elif t == "runtime" or t == "runtime_raw":
            payload = line.get("payload", {}) or {}
            ev_type = payload.get("event_type", "")
            if ev_type in ("tool_start", "tool_end"):
                tool_calls += 1
            if payload.get("tool_name") == "finalize_response":
                finalized = True
            if ev_type == "loop_iteration":
                loop_iters += 1

        elif t == "sub_agent_spawned":
            # Counts as a tool-level action
            tool_calls += 1

        elif t == "stream_event":
            ev = line.get("event", {})
            if ev.get("type") == "content_block_delta":
                delta = ev.get("delta", {})
                if delta.get("type") == "text_delta":
                    streamed_text += delta.get("text", "")

        elif t == "done":
            # New stream shape: completion is emitted as a "done" envelope.
            # Use reply/finalized fields when present.
            if bool(line.get("finalized")):
                finalized = True
            reply = line.get("reply", "")
            if isinstance(reply, str) and reply:
                final_answer_len = len(reply)
                streamed_text += reply

        elif t == "final_answer":
            payload = line.get("payload", {}) or {}
            msg = payload.get("message", "")
            # Some harness variants send a boolean marker instead of text.
            if isinstance(msg, str):
                msg_text = msg
            elif isinstance(msg, bool):
                msg_text = ""
                finalized = finalized or msg
            else:
                msg_text = str(msg) if msg is not None else ""
            if msg_text:
                final_answer_len = len(msg_text)
                finalized = True
                streamed_text += msg_text

    # Composite score
    s = 0.0
    if finalized:         s += 0.5
    if not error:         s += 0.15
    if tool_calls <= 8:   s += 0.1   # efficiency: fewer tools = less waste
    if tool_calls > 0:    s += 0.1   # at least used tools
    if len(streamed_text) >= 50: s += 0.15   # produced substantive output

    return {
        "finalized":        finalized,
        "final_answer_len": final_answer_len,
        "tool_calls":       tool_calls,
        "loop_iters":       loop_iters,
        "latency_s":        round(elapsed_s, 2),
        "error":            error,
        "phase_sequence":   phase_sequence,
        "streamed_chars":   len(streamed_text),
        "score":            round(s, 3),
    }

# ── Main ──────────────────────────────────────────────────────────────────────

async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--company-id", required=True)
    ap.add_argument("--persona", required=True)
    ap.add_argument("--prompt", required=True)
    ap.add_argument("--out", required=True, help="Path to write NDJSON log")
    ap.add_argument("--task-id", default="", help="Reuse existing task ID (skip creation)")
    ap.add_argument("--extra-notes", default="", help="JSON array of extra notes to prepend")
    args = ap.parse_args()

    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)

    extra_notes: list[dict] = []
    if args.extra_notes:
        extra_notes = json.loads(args.extra_notes)

    async with httpx.AsyncClient() as client:
        task_id = args.task_id
        if not task_id:
            print(f"[eval] Creating task for {args.persona}…", file=sys.stderr, flush=True)
            task_id = await create_task(
                client, args.company_id, args.persona,
                f"meta-harness eval · {args.persona} · {datetime.now(timezone.utc).strftime('%H:%M:%S')}"
            )
            print(f"[eval] Task: {task_id}", file=sys.stderr, flush=True)

        print(f"[eval] Streaming turn…", file=sys.stderr, flush=True)
        lines, elapsed = await stream_turn(
            client, task_id, args.persona, args.company_id, args.prompt, extra_notes
        )

    # Write raw NDJSON log
    with open(out_path, "w") as f:
        for line in lines:
            f.write(json.dumps(line) + "\n")

    metrics = score_turn(lines, elapsed)
    metrics["task_id"] = task_id
    metrics["persona"] = args.persona
    metrics["prompt"] = args.prompt[:120]
    metrics["ndjson_lines"] = len(lines)

    print(json.dumps(metrics, indent=2))
    return metrics


if __name__ == "__main__":
    asyncio.run(main())
