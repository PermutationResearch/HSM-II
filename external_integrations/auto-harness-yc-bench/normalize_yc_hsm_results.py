#!/usr/bin/env python3
"""
Normalize yc-bench (or wrapper) output into the flat map auto-harness expects:

  { "<task_id>": <float 0.0–1.0>, ... }

Default output path (used by ychsm_benchmark_runner.py): workspace/yc_hsm_results.json
Override with -o or env YCHSM_OUT.

Input shapes supported (auto-detected unless --format is set):
  - yc-bench rollout: {"transcript": [...], "terminal_reason": "horizon_end", ...}
    (written to results/yc_bench_result_<config>_<seed>_<modelslug>.json)
  - Flat JSON object: {"0": 1, "task-a": 0.5}
  - Wrapped: {"results": { ... }}
  - aggregate + per_task: {"per_task": {"t1": 1.0}}
  - List of records: [{"task_id": "a", "score": 0.8}, ...]
  - JSONL: one JSON object per line (same fields as list records)

Booleans are coerced: true -> 1.0, false -> 0.0
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional, Tuple


def _as_float_reward(v: Any) -> Optional[float]:
    if v is None:
        return None
    if isinstance(v, bool):
        return 1.0 if v else 0.0
    if isinstance(v, (int, float)):
        x = float(v)
        if x > 1.0 and x <= 100.0:
            return x / 100.0
        return max(0.0, min(1.0, x))
    return None


def _record_id_score(
    row: Dict[str, Any],
    id_keys: Tuple[str, ...],
    score_keys: Tuple[str, ...],
) -> Optional[Tuple[str, float]]:
    tid: Optional[str] = None
    for k in id_keys:
        if k in row and row[k] is not None:
            tid = str(row[k]).strip()
            break
    if not tid:
        return None
    for sk in score_keys:
        if sk in row:
            r = _as_float_reward(row[sk])
            if r is not None:
                return tid, r
    return None


def _from_flat_dict(d: Dict[str, Any]) -> Dict[str, float]:
    out: Dict[str, float] = {}
    for k, v in d.items():
        if k.startswith("_"):
            continue
        r = _as_float_reward(v)
        if r is not None:
            out[str(k)] = r
    return out


def _from_list_or_tasks(
    rows: Iterable[Dict[str, Any]],
    id_keys: Tuple[str, ...],
    score_keys: Tuple[str, ...],
) -> Dict[str, float]:
    out: Dict[str, float] = {}
    for row in rows:
        if not isinstance(row, dict):
            continue
        got = _record_id_score(row, id_keys, score_keys)
        if got:
            out[got[0]] = got[1]
    return out


def from_yc_bench_rollout(doc: Dict[str, Any]) -> Dict[str, float]:
    """
    Map yc-bench saved rollout to per-step + run-level rewards for auto-harness.

    - turn_<n>: fraction of commands in that turn with ok=true in the executor JSON
    - _run_terminal: 1.0 horizon_end, 0.0 bankruptcy/error, 0.5 otherwise
    """
    out: Dict[str, float] = {}
    tr = doc.get("transcript")
    if not isinstance(tr, list):
        return out

    for entry in tr:
        if not isinstance(entry, dict):
            continue
        tid = entry.get("turn")
        tid_s = str(tid) if tid is not None else str(len(out))
        cmds = entry.get("commands_executed") or []
        if not isinstance(cmds, list):
            cmds = []
        ok = 0
        for c in cmds:
            s = c if isinstance(c, str) else str(c)
            if '"ok": true' in s or '"ok":true' in s:
                ok += 1
        n = max(len(cmds), 1)
        out[f"turn_{tid_s}"] = ok / n

    term = doc.get("terminal_reason")
    if term == "bankruptcy":
        out["_run_terminal"] = 0.0
    elif term == "horizon_end":
        out["_run_terminal"] = 1.0
    elif term == "error":
        out["_run_terminal"] = 0.0
    else:
        out["_run_terminal"] = 0.5
    return out


def normalize_document(
    doc: Any,
    id_keys: Tuple[str, ...] = ("task_id", "id", "task", "name", "slug"),
    score_keys: Tuple[str, ...] = (
        "score",
        "reward",
        "pass_rate",
        "accuracy",
        "value",
        "passed",
        "success",
        "ok",
    ),
) -> Dict[str, float]:
    if doc is None:
        return {}

    if isinstance(doc, list):
        return _from_list_or_tasks(doc, id_keys, score_keys)

    if not isinstance(doc, dict):
        return {}

    if isinstance(doc.get("transcript"), list) and (
        "session_id" in doc or "terminal_reason" in doc or "turns_completed" in doc
    ):
        yc = from_yc_bench_rollout(doc)
        if yc:
            return yc

    if "results" in doc and isinstance(doc["results"], dict):
        inner = normalize_document(doc["results"], id_keys, score_keys)
        if inner:
            return inner

    if "per_task" in doc:
        pt = doc["per_task"]
        if isinstance(pt, dict):
            flat = _from_flat_dict(pt)
            if flat:
                return flat
        if isinstance(pt, list):
            got = _from_list_or_tasks(pt, id_keys, score_keys)
            if got:
                return got

    if "tasks" in doc and isinstance(doc["tasks"], list):
        got = _from_list_or_tasks(doc["tasks"], id_keys, score_keys)
        if got:
            return got

    flat = _from_flat_dict(doc)
    if flat:
        return flat

    return {}


def load_jsonl(path: Path, id_keys: Tuple[str, ...], score_keys: Tuple[str, ...]) -> Dict[str, float]:
    out: Dict[str, float] = {}
    text = path.read_text(encoding="utf-8", errors="replace")
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(row, dict):
            got = _record_id_score(row, id_keys, score_keys)
            if got:
                out[got[0]] = got[1]
    return out


def load_input(path: Optional[Path], fmt: str, id_keys: Tuple[str, ...], score_keys: Tuple[str, ...]) -> Dict[str, float]:
    if path is None or str(path) == "-":
        raw = sys.stdin.read()
    else:
        raw = path.read_text(encoding="utf-8", errors="replace")

    if fmt == "jsonl":
        lines = [ln for ln in raw.splitlines() if ln.strip()]
        rows: List[Dict[str, Any]] = []
        for ln in lines:
            try:
                o = json.loads(ln)
                if isinstance(o, dict):
                    rows.append(o)
            except json.JSONDecodeError:
                continue
        return _from_list_or_tasks(rows, id_keys, score_keys)

    try:
        doc = json.loads(raw)
    except json.JSONDecodeError as e:
        raise SystemExit(f"invalid JSON: {e}") from e

    if fmt == "flat":
        return _from_flat_dict(doc) if isinstance(doc, dict) else {}

    if fmt == "yc_rollout":
        return from_yc_bench_rollout(doc) if isinstance(doc, dict) else {}

    return normalize_document(doc, id_keys, score_keys)


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "-i",
        "--input",
        help="Input file (.json or .jsonl). Omit or '-' for stdin.",
    )
    p.add_argument(
        "-o",
        "--output",
        help="Output JSON path (default: workspace/yc_hsm_results.json or YCHSM_OUT).",
    )
    p.add_argument(
        "--format",
        choices=("auto", "jsonl", "flat", "yc_rollout"),
        default="auto",
        help="Parsing mode (default: auto). Use yc_rollout for results/yc_bench_result_*.json.",
    )
    p.add_argument(
        "--id-key",
        action="append",
        dest="id_keys",
        help="Extra JSON keys to treat as task id (repeatable).",
    )
    p.add_argument(
        "--score-key",
        action="append",
        dest="score_keys",
        help="Extra JSON keys to treat as score (repeatable).",
    )
    p.add_argument(
        "--min-tasks",
        type=int,
        default=0,
        help="Exit 2 if fewer than this many tasks (catch empty normalizations).",
    )
    p.add_argument(
        "--fail-empty",
        action="store_true",
        help="Exit 3 if no tasks were extracted (after parsing).",
    )
    args = p.parse_args()

    default_id = ("task_id", "id", "task", "name", "slug")
    default_score = ("score", "reward", "pass_rate", "accuracy", "value", "passed", "success", "ok")
    id_keys = tuple((args.id_keys or []) + list(default_id))
    score_keys = tuple((args.score_keys or []) + list(default_score))

    in_path = Path(args.input) if args.input and args.input != "-" else None
    if in_path is not None and not in_path.is_file():
        raise SystemExit(f"input not found: {in_path}")

    out_path = Path(
        args.output
        or __import__("os").environ.get("YCHSM_OUT", "").strip()
        or "workspace/yc_hsm_results.json"
    )

    if args.format == "jsonl" and in_path is not None:
        merged = load_jsonl(in_path, id_keys, score_keys)
    elif args.format == "yc_rollout" and in_path is not None:
        raw = in_path.read_text(encoding="utf-8", errors="replace")
        doc = json.loads(raw)
        merged = from_yc_bench_rollout(doc) if isinstance(doc, dict) else {}
    else:
        merged = load_input(in_path, args.format, id_keys, score_keys)

    if args.fail_empty and not merged:
        print("normalize_yc_hsm_results: extracted 0 tasks", file=sys.stderr)
        raise SystemExit(3)

    if args.min_tasks and len(merged) < args.min_tasks:
        print(
            f"normalize_yc_hsm_results: only {len(merged)} tasks (min {args.min_tasks})",
            file=sys.stderr,
        )
        raise SystemExit(2)

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(merged, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"Wrote {len(merged)} tasks -> {out_path.resolve()}")


if __name__ == "__main__":
    main()
