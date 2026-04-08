#!/usr/bin/env python3
"""
List lowest-scoring turn_* tasks from workspace/train_results.json (yc_hsm aggregate or single run).

Optionally load yc-bench rollout JSON(s) and print a short transcript snippet per turn.

With --discover-rollout (no --seed): scans all matching results/*.json for this config+model
(like aggregate_existing) and, for each turn, shows a snippet from the seed where that turn
scored worst (ties → lower seed).

Usage (from auto-harness-hsm):
  python3 list_worst_turns.py
  python3 list_worst_turns.py --top 15 --below 0.5
  python3 list_worst_turns.py --rollout /path/to/yc_bench_result_....json
  python3 list_worst_turns.py --discover-rollout              # aggregate: worst-seed snippet per turn
  python3 list_worst_turns.py --discover-rollout --seed 7     # single-seed snippets only
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Dict, List, Optional, Tuple

# Run from auto-harness-hsm (loads experiment_config.yaml via runner).
sys.path.insert(0, str(Path(__file__).resolve().parent))
from ychsm_benchmark_runner import YcHsmBenchRunner, from_yc_bench_rollout  # noqa: E402


def _clip(s: str, max_len: int) -> str:
    s = s.replace("\n", " ").strip()
    if len(s) <= max_len:
        return s
    return s[: max_len - 3] + "..."


def load_train_results(path: Path) -> Dict[str, float]:
    if not path.is_file():
        raise FileNotFoundError(path)
    with open(path) as f:
        data = json.load(f)
    raw = data.get("results", data) if isinstance(data, dict) else {}
    out: Dict[str, float] = {}
    if not isinstance(raw, dict):
        return out
    for k, v in raw.items():
        ks = str(k)
        if ks.startswith("_"):
            continue
        if not ks.startswith("turn_"):
            continue
        try:
            out[ks] = float(v)
        except (TypeError, ValueError):
            continue
    return out


def parse_turn_index(turn_key: str) -> Optional[int]:
    m = re.match(r"^turn_(\d+)$", turn_key)
    return int(m.group(1)) if m else None


def _rollout_paths(runner: YcHsmBenchRunner, seed: Optional[int]) -> List[Tuple[int, Path]]:
    discovered = runner._discover_rollout_paths()
    if seed is not None:
        for s, p in discovered:
            if s == seed and p.is_file():
                return [(s, p)]
        slug = runner._model_slug()
        cfg = runner.hsm_config
        for cand in (
            runner.root / "results" / f"yc_bench_result_{cfg}_{seed}_{slug}.json",
            runner.root / "results" / f"yc_bench_result_{cfg}_{seed}_{slug.replace(':', '_')}.json",
        ):
            if cand.is_file():
                return [(seed, cand)]
        return []
    return discovered


def load_rollout_caches(paths: List[Tuple[int, Path]]) -> List[Tuple[int, Path, Dict[int, dict], Dict[str, float], str]]:
    """(seed, path, transcript_by_turn, flat_scores, terminal_reason)."""
    out: List[Tuple[int, Path, Dict[int, dict], Dict[str, float], str]] = []
    for seed, path in paths:
        if not path.is_file():
            continue
        with open(path) as f:
            doc = json.load(f)
        if not isinstance(doc, dict):
            continue
        by_turn = index_transcript_by_turn(doc)
        flat = from_yc_bench_rollout(doc)
        term = str(doc.get("terminal_reason") or "")
        out.append((seed, path, by_turn, flat, term))
    return out


def pick_worst_seed_for_turn(
    turn_key: str, caches: List[Tuple[int, Path, Dict[int, dict], Dict[str, float], str]]
) -> Optional[Tuple[int, Path, Dict[int, dict], float]]:
    """Return (seed, path, by_turn, per_seed_score) for minimum score across seeds."""
    best: Optional[Tuple[float, int, Path, Dict[int, dict]]] = None
    for seed, path, by_turn, flat, _term in caches:
        if turn_key not in flat:
            continue
        sc = float(flat[turn_key])
        cand = (sc, seed, path, by_turn)
        if best is None or sc < best[0] or (sc == best[0] and seed < best[1]):
            best = cand
    if best is None:
        return None
    sc, seed, path, by_turn = best
    return seed, path, by_turn, sc


def index_transcript_by_turn(doc: dict) -> Dict[int, dict]:
    out: Dict[int, dict] = {}
    tr = doc.get("transcript")
    if not isinstance(tr, list):
        return out
    for entry in tr:
        if not isinstance(entry, dict):
            continue
        tid = entry.get("turn")
        if tid is None:
            continue
        try:
            out[int(tid)] = entry
        except (TypeError, ValueError):
            continue
    return out


def format_entry_snippet(entry: dict, max_chars: int) -> str:
    priority = (
        "role",
        "content",
        "message",
        "text",
        "assistant",
        "user",
        "observation",
        "commands_executed",
        "tool_calls",
        "error",
    )
    lines: List[str] = []
    budget = max_chars
    for key in priority:
        if key not in entry or budget < 40:
            continue
        val = entry[key]
        chunk = _clip(json.dumps(val, default=str), min(budget, 400))
        lines.append(f"  {key}: {chunk}")
        budget -= len(chunk) + 4
    if not lines:
        lines.append("  " + _clip(json.dumps(entry, default=str), max_chars))
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description="List worst turn_* scores + optional rollout snippets.")
    parser.add_argument(
        "--train-results",
        type=Path,
        default=Path("workspace/train_results.json"),
        help="Path to train_results.json (default: workspace/train_results.json)",
    )
    parser.add_argument("--top", type=int, default=25, help="Max turns to show (default: 25)")
    parser.add_argument(
        "--below",
        type=float,
        default=0.5,
        help="Only turns with score < this (default: 0.5)",
    )
    parser.add_argument(
        "--rollout",
        type=Path,
        default=None,
        help="yc_bench_result_*.json for transcript snippets",
    )
    parser.add_argument(
        "--discover-rollout",
        action="store_true",
        help="Load rollouts from experiment_config.yaml + yc_bench_root/results. "
        "Default: all matching seeds (aggregate); snippet from worst-scoring seed per turn. "
        "Use --seed to restrict to one rollout file.",
    )
    parser.add_argument(
        "--seed",
        type=int,
        default=None,
        help="With --discover-rollout: only this seed's JSON (omit for aggregate / worst-seed snippets).",
    )
    parser.add_argument(
        "--snippet-chars",
        type=int,
        default=900,
        help="Approx max chars of snippet per turn (default: 900)",
    )
    parser.add_argument(
        "--no-snippet",
        action="store_true",
        help="Only print turn_id and score",
    )
    args = parser.parse_args()

    try:
        scores = load_train_results(args.train_results)
    except FileNotFoundError as e:
        print(f"ERROR: {e}", file=sys.stderr)
        print("Run: python3 benchmark.py --split train", file=sys.stderr)
        return 1

    if not scores:
        print("No turn_* keys in results.")
        return 1

    ranked: List[Tuple[str, float]] = sorted(scores.items(), key=lambda x: (x[1], x[0]))
    worst = [(k, v) for k, v in ranked if v < args.below][: args.top]
    if not worst:
        worst = ranked[: args.top]

    caches: List[Tuple[int, Path, Dict[int, dict], Dict[str, float], str]] = []
    single_by_turn: Dict[int, dict] = {}
    single_term = ""

    if args.rollout and not args.no_snippet:
        if args.rollout.is_file():
            with open(args.rollout) as f:
                doc = json.load(f)
            if isinstance(doc, dict):
                single_by_turn = index_transcript_by_turn(doc)
                single_term = str(doc.get("terminal_reason") or "")
        else:
            print(f"WARN: rollout not found: {args.rollout}", file=sys.stderr)

    if args.discover_rollout and not args.no_snippet:
        runner = YcHsmBenchRunner(split="train")
        paths = _rollout_paths(runner, args.seed)
        caches = load_rollout_caches(paths)
        if args.seed is None:
            print(
                f"[list_worst_turns] aggregate snippets: {len(caches)} rollout(s) "
                f"{[c[0] for c in caches]}\n",
                flush=True,
            )
        elif not caches:
            print(
                f"[list_worst_turns] WARN: no rollout for seed={args.seed} — snippets skipped\n",
                file=sys.stderr,
                flush=True,
            )
        else:
            print(f"[list_worst_turns] single seed rollout: {caches[0][1]}\n", flush=True)

    print(f"source: {args.train_results.resolve()}")
    print(f"turns scored: {len(scores)}  |  showing up to {len(worst)}  |  threshold < {args.below}")
    if single_term:
        print(f"rollout terminal_reason: {single_term}")
    print()

    for tid, sc in worst:
        print(f"{tid}  merged_train_score={sc:.4f}")
        if args.no_snippet:
            print()
            continue

        if args.rollout:
            idx = parse_turn_index(tid)
            entry = single_by_turn.get(idx) if idx is not None else None
            if not entry:
                print("  (no transcript entry for this turn index)\n")
                continue
            print(format_entry_snippet(entry, args.snippet_chars))
            print()
            continue

        if caches:
            picked = pick_worst_seed_for_turn(tid, caches)
            if not picked:
                print("  (turn missing from all discovered rollouts)\n")
                continue
            seed, _path, by_turn, per_seed = picked
            idx = parse_turn_index(tid)
            entry = by_turn.get(idx) if idx is not None else None
            print(f"  snippet_seed={seed}  per_seed_score={per_seed:.4f}")
            if not entry:
                print("  (no transcript entry for this turn index)\n")
                continue
            print(format_entry_snippet(entry, args.snippet_chars))
            print()
            continue

        print()

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
