#!/usr/bin/env python3
"""
Summarize every HSM marketplace YC-bench rollout on disk (all companies / configs).

Uses the same flattening + val_score rules as ychsm_benchmark_runner.from_yc_bench_rollout:
  per-seed val_score = mean of keys not starting with '_'
  per-config aggregate = mean of per-seed val_scores (like aggregate_existing)
  portfolio_mean = unweighted mean of per-config aggregates

Does not run yc-bench — only reads results/yc_bench_result_*.json.

Usage (from auto-harness-hsm):
  python3 summarize_yc_bench_market_grid.py
  python3 summarize_yc_bench_market_grid.py --model openrouter/qwen/qwen3.6-plus:free
  YC_BENCH_ROOT=~/yc-bench python3 summarize_yc_bench_market_grid.py --json-out workspace/all_companies_summary.json
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any, DefaultDict, Dict, List, Optional, Tuple

# Import scoring from sibling module (run from this directory).
sys.path.insert(0, str(Path(__file__).resolve().parent))
from ychsm_benchmark_runner import from_yc_bench_rollout  # noqa: E402


def _val_score(flat: Dict[str, float]) -> float:
    filtered = {k: v for k, v in flat.items() if not str(k).startswith("_")}
    if not filtered:
        return 0.0
    return sum(filtered.values()) / len(filtered)


def _slug_variants(model: str) -> Tuple[str, str]:
    slug = model.replace("/", "_")
    return slug, slug.replace(":", "_")


def parse_result_filename(name: str) -> Optional[Tuple[str, int, str]]:
    """Return (yc_config, seed, model_slug_suffix) or None."""
    if not name.startswith("yc_bench_result_") or not name.endswith(".json"):
        return None
    body = name[len("yc_bench_result_") : -len(".json")]
    matches = list(re.finditer(r"_(\d+)_", body))
    if not matches:
        return None
    last = matches[-1]
    seed = int(last.group(1))
    config = body[: last.start()]
    slug = body[last.end() :]
    return config, seed, slug


def slug_matches(rest: str, slug: str, slug_alt: str) -> bool:
    return rest == slug or rest == slug_alt


def main() -> int:
    parser = argparse.ArgumentParser(description="Summarize all hsm_market_* YC-bench results for one model.")
    parser.add_argument(
        "--root",
        default=os.environ.get("YC_BENCH_ROOT", ""),
        help="yc-bench checkout (default: YC_BENCH_ROOT)",
    )
    parser.add_argument(
        "--model",
        default=os.environ.get("YC_BENCH_MODEL", "openrouter/qwen/qwen3.6-plus:free"),
        help="Model id as passed to yc-bench (default: env YC_BENCH_MODEL or qwen :free)",
    )
    parser.add_argument(
        "--config-prefix",
        default="hsm_market_",
        help="Only include yc-bench configs starting with this (default: hsm_market_)",
    )
    parser.add_argument(
        "--json-out",
        default="",
        help="Write full report JSON to this path (default: only print)",
    )
    args = parser.parse_args()
    root = (args.root or "").strip()
    if not root:
        print("ERROR: set --root or YC_BENCH_ROOT", file=sys.stderr)
        return 1
    results_dir = Path(root).resolve() / "results"
    if not results_dir.is_dir():
        print(f"ERROR: not a directory: {results_dir}", file=sys.stderr)
        return 1

    slug, slug_alt = _slug_variants(args.model)
    prefix = args.config_prefix

    # config -> seed -> val_score, plus turn counts
    by_cfg_seed: DefaultDict[str, Dict[int, float]] = defaultdict(dict)
    by_cfg_seed_turns: DefaultDict[str, Dict[int, int]] = defaultdict(dict)

    for p in results_dir.iterdir():
        if not p.is_file():
            continue
        parsed = parse_result_filename(p.name)
        if not parsed:
            continue
        cfg, seed, rest = parsed
        if not cfg.startswith(prefix):
            continue
        if not slug_matches(rest, slug, slug_alt):
            continue
        try:
            with open(p) as f:
                doc = json.load(f)
        except (OSError, json.JSONDecodeError) as e:
            print(f"skip {p.name}: {e}", file=sys.stderr)
            continue
        if not isinstance(doc, dict):
            continue
        flat = from_yc_bench_rollout(doc)
        if not flat:
            continue
        vs = _val_score(flat)
        n_turns = sum(1 for k in flat if not str(k).startswith("_"))
        by_cfg_seed[cfg][seed] = vs
        by_cfg_seed_turns[cfg][seed] = n_turns

    if not by_cfg_seed:
        print(f"No matching files under {results_dir} (prefix={prefix!r}, model slug ~ {slug!r}).")
        return 1

    rows: List[Dict[str, Any]] = []
    config_means: List[float] = []

    for cfg in sorted(by_cfg_seed.keys()):
        seeds_map = by_cfg_seed[cfg]
        seed_vals = [seeds_map[s] for s in sorted(seeds_map.keys())]
        mean_c = sum(seed_vals) / len(seed_vals) if seed_vals else 0.0
        config_means.append(mean_c)
        rows.append(
            {
                "yc_config": cfg,
                "company_pack": cfg[len(prefix) :] if cfg.startswith(prefix) else cfg,
                "seeds": sorted(seeds_map.keys()),
                "n_seeds": len(seeds_map),
                "per_seed_val_score": {str(s): round(seeds_map[s], 4) for s in sorted(seeds_map.keys())},
                "aggregate_val_score": round(mean_c, 4),
                "per_seed_n_turns": {str(s): by_cfg_seed_turns[cfg][s] for s in sorted(seeds_map.keys())},
            }
        )

    portfolio = sum(config_means) / len(config_means) if config_means else 0.0

    # Human table
    print(f"yc-bench results: {results_dir}\nmodel: {args.model}\n")
    print(f"{'company (pack)':<42} {'n':>3} {'agg_val':>8}  seeds")
    print("-" * 90)
    for r in rows:
        pack = r["company_pack"]
        line = f"{pack:<42} {r['n_seeds']:>3} {r['aggregate_val_score']:>8.4f}  {r['seeds']}"
        print(line)
    print("-" * 90)
    print(f"{'PORTFOLIO (mean of company aggregates)':<42} {len(rows):>3} {portfolio:>8.4f}")

    if args.json_out:
        out_path = Path(args.json_out)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        report = {
            "yc_bench_root": str(Path(root).resolve()),
            "model": args.model,
            "config_prefix": prefix,
            "portfolio_mean_val_score": round(portfolio, 4),
            "n_companies": len(rows),
            "companies": rows,
        }
        with open(out_path, "w") as f:
            json.dump(report, f, indent=2)
            f.write("\n")
        print(f"\nWrote {out_path}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
