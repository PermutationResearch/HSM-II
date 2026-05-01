"""
meta_harness.py — Automated search over the Company OS agent-chat harness.

Applies the Meta-Harness framework (Lee et al., 2026) to HSM-II:
https://arxiv.org/abs/2603.28052

Loop:
  1. Load current harness candidate (starts from baseline_v0)
  2. Run search-set evaluation (N eval turns)
  3. Compute aggregate metrics
  4. Emit proposer context (logs + current harness code snippets)
  5. Claude Code (you) reads, proposes harness edits → new candidate
  6. Repeat

Usage:
    python meta_harness.py --company-id <id> --iterations 3
    python meta_harness.py --company-id <id> --single-turn   # smoke test
    python meta_harness.py --single-turn --min-mean-score 0.6 --min-finalize-rate 1.0  # CI-style gate

State directory: HSM_META_HARNESS_DATA_DIR, else ~/.hsm/meta-harness when writable,
else <repo>/.meta-harness (sandbox/CI).
"""

import argparse
import asyncio
import json
import os
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Tuple

# ── Paths ─────────────────────────────────────────────────────────────────────

REPO_ROOT = Path(__file__).resolve().parents[2]
HARNESS_ROOT = REPO_ROOT / "web/company-console/app/lib"


def _resolve_meta_harness_root() -> Tuple[Path, str]:
    """
    Pick a writable directory for logs, candidates, and frontier state.

    Order:
      1. HSM_META_HARNESS_DATA_DIR if set (explicit override; CI / team policy).
      2. ~/.hsm/meta-harness when home is writable (default on real machines).
      3. <repo>/.meta-harness — sandbox- and CI-friendly (stays inside workspace).

    Returns (root_path, source_label) for status printing.
    """
    env = os.environ.get("HSM_META_HARNESS_DATA_DIR", "").strip()
    if env:
        root = Path(env).expanduser().resolve()
        root.mkdir(parents=True, exist_ok=True)
        return root, "HSM_META_HARNESS_DATA_DIR"

    legacy = Path.home() / ".hsm/meta-harness"
    try:
        legacy.mkdir(parents=True, exist_ok=True)
        probe = legacy / ".write_probe"
        probe.write_text("ok", encoding="utf-8")
        probe.unlink(missing_ok=True)
        return legacy, "~/.hsm/meta-harness"
    except OSError:
        pass

    local = (REPO_ROOT / ".meta-harness").resolve()
    local.mkdir(parents=True, exist_ok=True)
    return local, "repo .meta-harness (home not writable; e.g. sandbox)"


META_HARNESS_ROOT, META_HARNESS_ROOT_SOURCE = _resolve_meta_harness_root()
LOGS_DIR = META_HARNESS_ROOT / "logs"
CANDIDATES_DIR = META_HARNESS_ROOT / "candidates"
FRONTIER_FILE = META_HARNESS_ROOT / "frontier.json"
EVOLUTION_LOG = META_HARNESS_ROOT / "evolution_log.jsonl"

for d in (LOGS_DIR, CANDIDATES_DIR):
    d.mkdir(parents=True, exist_ok=True)

# ── Eval task set ─────────────────────────────────────────────────────────────
# Each entry: {persona, prompt, label}
# Covers: research, engineering, coordination, stigmergic, analysis

EVAL_TASKS = [
    # Engineering / tool-heavy
    {
        "persona": "cto",
        "prompt": "run repo-intel on this codebase and summarize the top 3 architecture risks",
        "label": "eng_repo_intel",
    },
    {
        "persona": "staff-engineer",
        "prompt": "run validate-delivery on the latest changes and tell me if we're ready to ship",
        "label": "eng_validate_delivery",
    },
    {
        "persona": "cto",
        "prompt": "run orchestrate-review on the recent PRs and flag anything blocking",
        "label": "eng_review",
    },
    # Research / synthesis
    {
        "persona": "research-perf-analyst",
        "prompt": "use your research skills to give me a brief on the current state of agent harness optimization papers",
        "label": "research_harness_papers",
    },
    # Planning / coordination
    {
        "persona": "ceo",
        "prompt": "what are the top 3 priorities for agentsys engineering this week based on current task state?",
        "label": "coord_priorities",
    },
    # Quick operational
    {
        "persona": "qa-release-lead",
        "prompt": "run validate-delivery and give me a pass/fail verdict",
        "label": "qa_validate",
    },
    # Stigmergic / memory
    {
        "persona": "staff-engineer",
        "prompt": "what have you been working on recently? summarize from your notes",
        "label": "stigmergic_recall",
    },
    # Cross-cutting analysis
    {
        "persona": "cto",
        "prompt": "run perf-analyzer and give me the top bottleneck to address",
        "label": "eng_perf",
    },
]

# ── Harness snapshots ─────────────────────────────────────────────────────────

HARNESS_FILES = [
    HARNESS_ROOT / "agent-chat-stream-server.ts",
]

def snapshot_harness(candidate_dir: Path):
    """Copy current harness files into the candidate directory."""
    for f in HARNESS_FILES:
        if f.exists():
            dest = candidate_dir / f.name
            dest.write_text(f.read_text())

def read_harness_excerpt() -> str:
    """Return key sections of the current harness for proposer context."""
    excerpts = []
    for f in HARNESS_FILES:
        if not f.exists():
            continue
        text = f.read_text()
        lines = text.splitlines()
        # Grab first 80 lines (imports + constants) and buildSystemPrompt/workerCompanionSystemPrompt
        header = "\n".join(lines[:80])
        # Find buildSystemPrompt
        for i, l in enumerate(lines):
            if "buildSystemPrompt" in l or "workerCompanionSystemPrompt" in l or "buildCompactedContextBundle" in l:
                chunk = "\n".join(lines[max(0, i-2):min(len(lines), i+20)])
                excerpts.append(f"// ~line {i} in {f.name}:\n{chunk}")
        excerpts.insert(0, f"// Header of {f.name}:\n{header}")
    return "\n\n---\n\n".join(excerpts)

# ── Single-turn evaluation ─────────────────────────────────────────────────────

async def run_eval_turn(company_id: str, task: dict, candidate_id: str, turn_idx: int) -> dict:
    """Run one eval task, write NDJSON log, return metrics."""
    out_dir = CANDIDATES_DIR / candidate_id / "results"
    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = out_dir / f"{task['label']}.ndjson"

    eval_script = Path(__file__).parent / "evaluate_turn.py"

    cmd = [
        sys.executable, str(eval_script),
        "--company-id", company_id,
        "--persona", task["persona"],
        "--prompt", task["prompt"],
        "--out", str(out_path),
    ]

    print(f"  [{turn_idx+1:02d}] {task['label']} ({task['persona']})…", end=" ", flush=True)
    t0 = time.monotonic()

    proc = await asyncio.create_subprocess_exec(
        *cmd,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    stdout, stderr = await proc.communicate()
    elapsed = time.monotonic() - t0

    if proc.returncode != 0:
        print(f"❌ ({elapsed:.1f}s)")
        if stderr:
            print(f"     stderr: {stderr.decode()[:300]}", flush=True)
        return {
            "label": task["label"],
            "persona": task["persona"],
            "error": True,
            "score": 0.0,
            "latency_s": elapsed,
        }

    try:
        # evaluate_turn.py prints a single JSON object to stdout (possibly pretty-printed)
        raw = stdout.decode().strip()
        metrics = json.loads(raw)
    except (json.JSONDecodeError, IndexError, ValueError):
        # Fallback: try to extract last {...} block
        try:
            start = raw.rfind("{")
            end = raw.rfind("}") + 1
            metrics = json.loads(raw[start:end]) if start >= 0 and end > start else {"score": 0.0, "error": True}
        except Exception:
            metrics = {"score": 0.0, "error": True}

    metrics["label"] = task["label"]
    metrics["persona"] = task["persona"]

    score = metrics.get("score", 0.0)
    fin = "✅" if metrics.get("finalized") else "⚠️"
    bs = metrics.get("belief_state") or {}
    ts = bs.get("task_success") or {}
    p_mean = ts.get("posterior_mean")
    belief_s = f" belief={p_mean:.2f}" if isinstance(p_mean, (int, float)) else ""
    print(
        f"{fin} score={score:.2f} latency={metrics.get('latency_s',0):.1f}s "
        f"tools={metrics.get('tool_calls',0)}{belief_s}",
        flush=True,
    )

    return metrics

# ── Aggregate scoring ─────────────────────────────────────────────────────────

def aggregate(results: list[dict]) -> dict:
    scores = [r.get("score", 0.0) for r in results]
    finalized = [r.get("finalized", False) for r in results]
    latencies = [r.get("latency_s", 0.0) for r in results]
    tool_counts = [r.get("tool_calls", 0) for r in results]
    errors = [r.get("error", False) for r in results]

    n = len(results) or 1
    return {
        "n":                n,
        "mean_score":       round(sum(scores) / n, 3),
        "finalize_rate":    round(sum(finalized) / n, 3),
        "error_rate":       round(sum(errors) / n, 3),
        "mean_latency_s":   round(sum(latencies) / n, 2),
        "mean_tool_calls":  round(sum(tool_counts) / n, 2),
        "scores":           scores,
    }

# ── Proposer context ─────────────────────────────────────────────────────────

def build_proposer_context(candidate_id: str, agg: dict, results: list[dict]) -> str:
    """Render the text block Claude reads as proposer to produce the next harness edit."""
    weak = [r for r in results if r.get("score", 1.0) < 0.5]
    weak_summary = "\n".join(
        f"  - {r['label']} ({r['persona']}): score={r.get('score',0):.2f} "
        f"finalized={r.get('finalized')} tools={r.get('tool_calls',0)} err={r.get('error')}"
        for r in weak
    ) or "  (none)"

    harness_excerpt = read_harness_excerpt()

    return f"""
=== Meta-Harness Proposer Context — Candidate {candidate_id} ===

Aggregate metrics:
  mean_score:     {agg['mean_score']}
  finalize_rate:  {agg['finalize_rate']}   ← fraction of turns where finalize_response fired
  error_rate:     {agg['error_rate']}
  mean_latency_s: {agg['mean_latency_s']}
  mean_tool_calls:{agg['mean_tool_calls']}

Weak turns (score < 0.5):
{weak_summary}

Per-turn breakdown:
{json.dumps(results, indent=2)}

Current harness code excerpts:
{harness_excerpt}

=== Task for proposer (Claude Code) ===

Based on the above metrics and harness code, propose ONE targeted edit to improve the harness.
Focus on the highest-impact lever first:
  1. System prompt / persona framing  → edit `buildSystemPrompt()` or `workerCompanionSystemPrompt()`
  2. Context injection                → edit `buildCompactedContextBundle()` parameters
  3. Skill detection sensitivity      → edit `detectSkillDispatch()` patterns
  4. Loop guard / finalize trigger    → edit max_iterations or finalize_response criteria in Rust

Constraints:
  - Change ONE thing at a time
  - Edits to .ts files take effect immediately (no rebuild)
  - Edits to .rs files require `cargo build --bin hsm_console` in the worktree
  - State your hypothesis before making the edit
  - State the expected metric improvement

Write the edit now.
"""

# ── Evolution log ─────────────────────────────────────────────────────────────

def log_candidate(candidate_id: str, agg: dict, harness_desc: str):
    entry = {
        "candidate_id": candidate_id,
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "aggregate": agg,
        "harness_desc": harness_desc,
    }
    with open(EVOLUTION_LOG, "a") as f:
        f.write(json.dumps(entry) + "\n")

def load_frontier() -> list[dict]:
    if FRONTIER_FILE.exists():
        return json.loads(FRONTIER_FILE.read_text())
    return []

def update_frontier(candidate_id: str, agg: dict):
    frontier = load_frontier()
    frontier.append({
        "candidate_id": candidate_id,
        "mean_score": agg["mean_score"],
        "finalize_rate": agg["finalize_rate"],
        "mean_latency_s": agg["mean_latency_s"],
        "mean_tool_calls": agg["mean_tool_calls"],
    })
    # Keep Pareto-optimal: dominated entries removed
    # (higher score AND lower latency)
    pareto = []
    for c in frontier:
        dominated = any(
            o["mean_score"] >= c["mean_score"] and o["mean_latency_s"] <= c["mean_latency_s"]
            and o["candidate_id"] != c["candidate_id"]
            for o in frontier
        )
        if not dominated:
            pareto.append(c)
    FRONTIER_FILE.write_text(json.dumps(pareto, indent=2))
    return pareto

# ── Main ──────────────────────────────────────────────────────────────────────

async def run_iteration(company_id: str, candidate_id: str, tasks: list[dict]) -> dict:
    print(f"\n{'='*60}")
    print(f"Evaluating candidate: {candidate_id}")
    print(f"Tasks: {len(tasks)}")
    print(f"{'='*60}")

    # Snapshot current harness
    cand_dir = CANDIDATES_DIR / candidate_id
    cand_dir.mkdir(parents=True, exist_ok=True)
    snapshot_harness(cand_dir)

    results = []
    for i, task in enumerate(tasks):
        result = await run_eval_turn(company_id, task, candidate_id, i)
        results.append(result)

    # Write results summary
    (cand_dir / "results.json").write_text(json.dumps(results, indent=2))

    agg = aggregate(results)
    (cand_dir / "summary.json").write_text(json.dumps(agg, indent=2))

    print(f"\n--- Summary: candidate {candidate_id} ---")
    print(f"  mean_score:     {agg['mean_score']}")
    print(f"  finalize_rate:  {agg['finalize_rate']}")
    print(f"  error_rate:     {agg['error_rate']}")
    print(f"  mean_latency_s: {agg['mean_latency_s']}")
    print(f"  mean_tool_calls:{agg['mean_tool_calls']}")

    return {"agg": agg, "results": results}


async def main():
    ap = argparse.ArgumentParser(description="Meta-Harness for Company OS agent-chat")
    ap.add_argument("--company-id", default="0b1aeb33-4f4e-4c70-8d83-a66d087e24c5",
                    help="Company OS company ID (default: agentsys-engineering)")
    ap.add_argument("--iterations", type=int, default=1,
                    help="Number of propose+evaluate cycles")
    ap.add_argument("--single-turn", action="store_true",
                    help="Smoke test: run only the first eval task")
    ap.add_argument("--tasks", type=int, default=len(EVAL_TASKS),
                    help=f"Number of eval tasks to run (max {len(EVAL_TASKS)})")
    ap.add_argument("--candidate-id", default="",
                    help="Override candidate ID (default: auto-generated)")
    ap.add_argument("--min-mean-score", type=float, default=-1.0,
                    help="If >= 0, exit with status 1 when the last iteration's mean_score is below this")
    ap.add_argument("--min-finalize-rate", type=float, default=-1.0,
                    help="If >= 0, exit with status 1 when the last iteration's finalize_rate is below this")
    args = ap.parse_args()

    tasks = EVAL_TASKS[:1] if args.single_turn else EVAL_TASKS[:args.tasks]
    company_id = args.company_id

    print(f"Meta-Harness — Company OS agent-chat")
    print(f"Company:    {company_id}")
    print(f"Iterations: {args.iterations}")
    print(f"Tasks/iter: {len(tasks)}")
    print(f"Data root:  {META_HARNESS_ROOT}  ({META_HARNESS_ROOT_SOURCE})")
    print(f"Logs:       {LOGS_DIR}")
    print(f"Candidates: {CANDIDATES_DIR}")

    last_agg = None
    for iteration in range(args.iterations):
        candidate_id = args.candidate_id or f"iter{iteration:02d}_{datetime.now(timezone.utc).strftime('%H%M%S')}"

        data = await run_iteration(company_id, candidate_id, tasks)
        agg = data["agg"]
        results = data["results"]
        last_agg = agg

        log_candidate(candidate_id, agg, "current harness")
        pareto = update_frontier(candidate_id, agg)

        print(f"\nPareto frontier ({len(pareto)} candidates):")
        for p in pareto:
            print(f"  {p['candidate_id']}: score={p['mean_score']} latency={p['mean_latency_s']}s")

        if iteration < args.iterations - 1:
            # Emit proposer context for Claude Code to act on
            ctx = build_proposer_context(candidate_id, agg, results)
            proposer_file = CANDIDATES_DIR / candidate_id / "proposer_context.md"
            proposer_file.write_text(ctx)
            print(f"\n[proposer] Context written to: {proposer_file}")
            print("[proposer] Read it, make harness edits, then the next iteration will evaluate.")
            print(ctx)
            input("\n[meta-harness] Press ENTER after making harness edits to run next iteration…")

    print("\n✅ Meta-Harness run complete.")
    print(f"Evolution log: {EVOLUTION_LOG}")
    print(f"Frontier: {FRONTIER_FILE}")

    if last_agg is not None:
        if args.min_mean_score >= 0.0 and last_agg["mean_score"] < args.min_mean_score:
            print(
                f"\n❌ Quality gate: mean_score {last_agg['mean_score']:.3f} < {args.min_mean_score}",
                file=sys.stderr,
            )
            sys.exit(1)
        if args.min_finalize_rate >= 0.0 and last_agg["finalize_rate"] < args.min_finalize_rate:
            print(
                f"\n❌ Quality gate: finalize_rate {last_agg['finalize_rate']:.3f} < {args.min_finalize_rate}",
                file=sys.stderr,
            )
            sys.exit(1)


if __name__ == "__main__":
    asyncio.run(main())
