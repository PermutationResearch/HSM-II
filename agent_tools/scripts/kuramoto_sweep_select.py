#!/usr/bin/env python3
import argparse
import statistics
from pathlib import Path

from kuramoto_protocol_eval import (
    bootstrap_ci,
    bootstrap_ci_paired,
    first_tick_at_or_above,
    load_run_metrics,
    pair_runs,
    pct_change,
    pct_reduction,
    summarize,
)


def find_candidates(root: Path, baseline_name: str):
    candidates = []
    for path in sorted(root.glob("experiments_kura_*")):
        if not path.is_dir():
            continue
        if path.name == baseline_name:
            continue
        if "baseline" in path.name:
            continue
        if any(path.glob("run_*/*_summary.json")):
            candidates.append(path)
    return candidates


def evaluate_candidate(base_rows, baseline_summary, candidate_rows):
    abs_target = baseline_summary["final_coh_median"]
    for r in base_rows:
        r["conv_abs"] = first_tick_at_or_above(r, abs_target)
    for r in candidate_rows:
        r["conv_abs"] = first_tick_at_or_above(r, abs_target)

    cand_summary = summarize(candidate_rows)
    out = {
        "n": cand_summary["n"],
        "conv90_impr": pct_reduction(
            baseline_summary["conv90_median"], cand_summary["conv90_median"]
        ),
        "conv_abs_impr": pct_reduction(
            statistics.median(r["conv_abs"] for r in base_rows),
            statistics.median(r["conv_abs"] for r in candidate_rows),
        ),
        "disagree_red": pct_reduction(
            baseline_summary["disagree_mean"], cand_summary["disagree_mean"]
        ),
        "coh_delta": pct_change(
            cand_summary["final_coh_mean"], baseline_summary["final_coh_mean"]
        ),
        "rew_delta": pct_change(cand_summary["reward_mean"], baseline_summary["reward_mean"]),
    }

    out["conv_abs_ci"] = bootstrap_ci(
        base_rows,
        candidate_rows,
        lambda b, t: pct_reduction(
            statistics.median(r["conv_abs"] for r in b),
            statistics.median(r["conv_abs"] for r in t),
        ),
    )
    out["disagree_ci"] = bootstrap_ci(
        base_rows,
        candidate_rows,
        lambda b, t: pct_reduction(
            statistics.mean(r["disagreement_rate"] for r in b),
            statistics.mean(r["disagreement_rate"] for r in t),
        ),
    )
    out["coh_ci"] = bootstrap_ci(
        base_rows,
        candidate_rows,
        lambda b, t: pct_change(
            statistics.mean(r["final_coherence"] for r in t),
            statistics.mean(r["final_coherence"] for r in b),
        ),
    )
    out["rew_ci"] = bootstrap_ci(
        base_rows,
        candidate_rows,
        lambda b, t: pct_change(
            statistics.mean(r["mean_reward_per_tick"] for r in t),
            statistics.mean(r["mean_reward_per_tick"] for r in b),
        ),
    )

    pairs, pair_key = pair_runs(base_rows, candidate_rows)
    if pairs:
        out["pairs"] = len(pairs)
        out["pair_key"] = pair_key
        out["pair_conv_abs_impr"] = statistics.mean(
            pct_reduction(b["conv_abs"], t["conv_abs"]) for b, t in pairs
        )
        out["pair_disagree_red"] = statistics.mean(
            pct_reduction(b["disagreement_rate"], t["disagreement_rate"]) for b, t in pairs
        )
        out["pair_coh_delta"] = statistics.mean(
            pct_change(t["final_coherence"], b["final_coherence"]) for b, t in pairs
        )
        out["pair_rew_delta"] = statistics.mean(
            pct_change(t["mean_reward_per_tick"], b["mean_reward_per_tick"])
            for b, t in pairs
        )
        out["pair_conv_abs_ci"] = bootstrap_ci_paired(
            pairs,
            lambda ps: statistics.mean(
                pct_reduction(b["conv_abs"], t["conv_abs"]) for b, t in ps
            ),
        )
        out["pair_disagree_ci"] = bootstrap_ci_paired(
            pairs,
            lambda ps: statistics.mean(
                pct_reduction(b["disagreement_rate"], t["disagreement_rate"])
                for b, t in ps
            ),
        )
        out["pair_coh_ci"] = bootstrap_ci_paired(
            pairs,
            lambda ps: statistics.mean(
                pct_change(t["final_coherence"], b["final_coherence"]) for b, t in ps
            ),
        )
        out["pair_rew_ci"] = bootstrap_ci_paired(
            pairs,
            lambda ps: statistics.mean(
                pct_change(t["mean_reward_per_tick"], b["mean_reward_per_tick"])
                for b, t in ps
            ),
        )
    else:
        out["pairs"] = 0
    return out


def pass_fail(eval_result, args):
    reasons = []
    conv_abs = eval_result.get("pair_conv_abs_impr", eval_result["conv_abs_impr"])
    disagree = eval_result.get("pair_disagree_red", eval_result["disagree_red"])
    coh = eval_result.get("pair_coh_delta", eval_result["coh_delta"])
    rew = eval_result.get("pair_rew_delta", eval_result["rew_delta"])

    if conv_abs < args.min_conv_abs_impr:
        reasons.append(
            f"conv_abs_impr {conv_abs:.2f}% < min {args.min_conv_abs_impr:.2f}%"
        )
    if disagree < args.min_disagreement_red:
        reasons.append(
            f"disagreement_red {disagree:.2f}% < min {args.min_disagreement_red:.2f}%"
        )
    if coh < args.min_coherence_delta:
        reasons.append(f"coherence_delta {coh:.2f}% < min {args.min_coherence_delta:.2f}%")
    if rew < args.min_reward_delta:
        reasons.append(f"reward_delta {rew:.2f}% < min {args.min_reward_delta:.2f}%")

    if args.require_ci_positive:
        conv_ci = eval_result.get("pair_conv_abs_ci", eval_result["conv_abs_ci"])
        dis_ci = eval_result.get("pair_disagree_ci", eval_result["disagree_ci"])
        if conv_ci[1] <= 0.0:
            reasons.append(
                f"conv_abs_ci_lower {conv_ci[1]:.2f}% <= 0.00% (not robustly positive)"
            )
        if dis_ci[1] <= 0.0:
            reasons.append(
                f"disagree_ci_lower {dis_ci[1]:.2f}% <= 0.00% (not robustly positive)"
            )

    return len(reasons) == 0, reasons


def main():
    p = argparse.ArgumentParser(
        description="Select Kuramoto sweep configs with hard non-inferiority constraints."
    )
    p.add_argument("--baseline", required=True, help="Baseline experiment directory")
    p.add_argument(
        "--candidate",
        action="append",
        default=[],
        help="Candidate treatment directory (repeatable). If omitted, auto-discovers experiments_kura_* dirs.",
    )
    p.add_argument(
        "--min-conv-abs-impr",
        type=float,
        default=0.0,
        help="Minimum absolute convergence improvement percent.",
    )
    p.add_argument(
        "--min-disagreement-red",
        type=float,
        default=0.0,
        help="Minimum disagreement reduction percent.",
    )
    p.add_argument(
        "--min-coherence-delta",
        type=float,
        default=0.0,
        help="Minimum final coherence delta percent (non-inferiority: 0).",
    )
    p.add_argument(
        "--min-reward-delta",
        type=float,
        default=0.0,
        help="Minimum reward delta percent (non-inferiority: 0).",
    )
    p.add_argument(
        "--require-ci-positive",
        action="store_true",
        help="Require lower CI bound > 0 for absolute convergence and disagreement improvements.",
    )
    args = p.parse_args()

    baseline_dir = Path(args.baseline)
    base_rows = load_run_metrics(baseline_dir)
    if not base_rows:
        raise SystemExit(f"No runs found in baseline: {baseline_dir}")
    baseline_summary = summarize(base_rows)

    if args.candidate:
        candidates = [Path(c) for c in args.candidate]
    else:
        candidates = find_candidates(Path("."), baseline_dir.name)
    if not candidates:
        raise SystemExit("No candidate directories found.")

    results = []
    for candidate in candidates:
        rows = load_run_metrics(candidate)
        if not rows:
            continue
        ev = evaluate_candidate(base_rows, baseline_summary, rows)
        ok, reasons = pass_fail(ev, args)
        results.append({"name": candidate.name, "ok": ok, "reasons": reasons, **ev})

    if not results:
        raise SystemExit("No valid candidate run data found.")

    winners = [r for r in results if r["ok"]]
    winners.sort(
        key=lambda r: (
            r.get("pair_conv_abs_impr", r["conv_abs_impr"]),
            r.get("pair_disagree_red", r["disagree_red"]),
            r.get("pair_rew_delta", r["rew_delta"]),
        ),
        reverse=True,
    )
    losers = [r for r in results if not r["ok"]]

    print("Baseline:", baseline_dir.name)
    print("Candidates tested:", ", ".join(r["name"] for r in results))
    print(
        "\nSelection thresholds:"
        f" conv_abs>={args.min_conv_abs_impr:.2f}%"
        f", disagreement>={args.min_disagreement_red:.2f}%"
        f", coherence>={args.min_coherence_delta:.2f}%"
        f", reward>={args.min_reward_delta:.2f}%"
        + (", require positive CI lower bounds" if args.require_ci_positive else "")
    )

    print("\nAccepted candidates (ranked):")
    if winners:
        for i, r in enumerate(winners, start=1):
            print(
                f"{i}. {r['name']}: "
                f"conv_abs={r.get('pair_conv_abs_impr', r['conv_abs_impr']):.2f}% "
                f"dis={r.get('pair_disagree_red', r['disagree_red']):.2f}% "
                f"coh={r.get('pair_coh_delta', r['coh_delta']):.2f}% "
                f"rew={r.get('pair_rew_delta', r['rew_delta']):.2f}% "
                f"pairs={r.get('pairs', 0)}"
            )
    else:
        print("  (none)")

    print("\nRejected candidates:")
    for r in losers:
        print(f"- {r['name']}: {'; '.join(r['reasons'])}")


if __name__ == "__main__":
    main()
