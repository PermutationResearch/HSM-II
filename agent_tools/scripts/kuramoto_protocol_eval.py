#!/usr/bin/env python3
import argparse
import csv
import glob
import json
import random
import statistics
from pathlib import Path


def load_run_metrics(experiment_dir: Path):
    out = []
    for summary_path in sorted(glob.glob(str(experiment_dir / "run_*" / "*_summary.json"))):
        with open(summary_path, "r", encoding="utf-8") as f:
            s = json.load(f)

        run_dir = Path(summary_path).parent
        run_id = run_dir.name
        seed = None
        name_parts = Path(summary_path).stem.split("_")
        if "seed" in name_parts:
            i = name_parts.index("seed")
            if i + 1 < len(name_parts):
                seed = name_parts[i + 1]

        snap_files = sorted(run_dir.glob("*_snapshots.csv"))
        if not snap_files:
            continue
        with open(snap_files[0], "r", encoding="utf-8") as f:
            rows = list(csv.DictReader(f))
        if not rows:
            continue

        ticks = [int(r["tick"]) for r in rows]
        coherences = [float(r["global_coherence"]) for r in rows]
        final_tick = ticks[-1]
        final_coh = coherences[-1]
        conv90 = next(
            (
                tick
                for tick, coh in zip(ticks, coherences)
                if coh >= 0.9 * final_coh
            ),
            final_tick,
        )
        conv80 = next(
            (
                tick
                for tick, coh in zip(ticks, coherences)
                if coh >= 0.8 * final_coh
            ),
            final_tick,
        )

        out.append(
            {
                "run_id": run_id,
                "seed": seed,
                "final_coherence": float(s["final_coherence"]),
                "mean_reward_per_tick": float(s["mean_reward_per_tick"]),
                "disagreement_rate": 1.0 - float(s["council_approve_rate"]),
                "skills_promoted": float(s["skills_promoted"]),
                "conv80": conv80,
                "conv90": conv90,
                "ticks": ticks,
                "coherences": coherences,
            }
        )
    return out


def summarize(rows):
    return {
        "n": len(rows),
        "final_coh_mean": statistics.mean(r["final_coherence"] for r in rows),
        "final_coh_median": statistics.median(r["final_coherence"] for r in rows),
        "reward_mean": statistics.mean(r["mean_reward_per_tick"] for r in rows),
        "reward_median": statistics.median(r["mean_reward_per_tick"] for r in rows),
        "disagree_mean": statistics.mean(r["disagreement_rate"] for r in rows),
        "skills_promoted_mean": statistics.mean(r["skills_promoted"] for r in rows),
        "conv80_median": statistics.median(r["conv80"] for r in rows),
        "conv90_median": statistics.median(r["conv90"] for r in rows),
    }


def bootstrap_ci(base_rows, treat_rows, metric_fn, samples=5000, seed=42):
    random.seed(seed)
    vals = []
    for _ in range(samples):
        b = [random.choice(base_rows) for _ in base_rows]
        t = [random.choice(treat_rows) for _ in treat_rows]
        vals.append(metric_fn(b, t))
    vals.sort()
    return vals[samples // 2], vals[int(samples * 0.025)], vals[int(samples * 0.975)]


def bootstrap_ci_paired(pairs, metric_fn, samples=5000, seed=42):
    random.seed(seed)
    vals = []
    for _ in range(samples):
        resampled = [random.choice(pairs) for _ in pairs]
        vals.append(metric_fn(resampled))
    vals.sort()
    return vals[samples // 2], vals[int(samples * 0.025)], vals[int(samples * 0.975)]


def pct_change(new_value, base_value):
    return ((new_value - base_value) / base_value * 100.0) if base_value else 0.0


def pct_reduction(base_value, new_value):
    return ((base_value - new_value) / base_value * 100.0) if base_value else 0.0


def first_tick_at_or_above(row, threshold):
    final_tick = row["ticks"][-1]
    for tick, coh in zip(row["ticks"], row["coherences"]):
        if coh >= threshold:
            return tick
    return final_tick


def pair_runs(base_rows, treat_rows):
    base_by_seed = {r["seed"]: r for r in base_rows if r["seed"] is not None}
    treat_by_seed = {r["seed"]: r for r in treat_rows if r["seed"] is not None}
    common_seeds = sorted(set(base_by_seed) & set(treat_by_seed))
    if common_seeds:
        return [(base_by_seed[s], treat_by_seed[s]) for s in common_seeds], "seed"

    base_by_run = {r["run_id"]: r for r in base_rows}
    treat_by_run = {r["run_id"]: r for r in treat_rows}
    common_runs = sorted(set(base_by_run) & set(treat_by_run))
    return [(base_by_run[r], treat_by_run[r]) for r in common_runs], "run_id"


def main():
    p = argparse.ArgumentParser(description="Evaluate Kuramoto validation protocol metrics")
    p.add_argument("--baseline", required=True, help="Baseline experiment directory")
    p.add_argument("--treatment", required=True, help="Treatment experiment directory")
    args = p.parse_args()

    base_rows = load_run_metrics(Path(args.baseline))
    treat_rows = load_run_metrics(Path(args.treatment))
    if not base_rows or not treat_rows:
        raise SystemExit("Missing baseline or treatment run summaries/snapshots.")

    bs = summarize(base_rows)
    ts = summarize(treat_rows)

    conv_impr = pct_reduction(bs["conv90_median"], ts["conv90_median"])
    dis_red = pct_reduction(bs["disagree_mean"], ts["disagree_mean"])
    coh_delta = pct_change(ts["final_coh_mean"], bs["final_coh_mean"])
    rew_delta = pct_change(ts["reward_mean"], bs["reward_mean"])

    c_conv = bootstrap_ci(
        base_rows,
        treat_rows,
        lambda b, t: pct_reduction(
            statistics.median(r["conv90"] for r in b),
            statistics.median(r["conv90"] for r in t),
        ),
    )
    c_dis = bootstrap_ci(
        base_rows,
        treat_rows,
        lambda b, t: pct_reduction(
            statistics.mean(r["disagreement_rate"] for r in b),
            statistics.mean(r["disagreement_rate"] for r in t),
        ),
    )
    c_coh = bootstrap_ci(
        base_rows,
        treat_rows,
        lambda b, t: pct_change(
            statistics.mean(r["final_coherence"] for r in t),
            statistics.mean(r["final_coherence"] for r in b),
        ),
    )
    c_rew = bootstrap_ci(
        base_rows,
        treat_rows,
        lambda b, t: pct_change(
            statistics.mean(r["mean_reward_per_tick"] for r in t),
            statistics.mean(r["mean_reward_per_tick"] for r in b),
        ),
    )

    abs_target = bs["final_coh_median"]
    for r in base_rows:
        r["conv_abs"] = first_tick_at_or_above(r, abs_target)
    for r in treat_rows:
        r["conv_abs"] = first_tick_at_or_above(r, abs_target)
    abs_conv_impr = pct_reduction(
        statistics.median(r["conv_abs"] for r in base_rows),
        statistics.median(r["conv_abs"] for r in treat_rows),
    )
    c_abs_conv = bootstrap_ci(
        base_rows,
        treat_rows,
        lambda b, t: pct_reduction(
            statistics.median(r["conv_abs"] for r in b),
            statistics.median(r["conv_abs"] for r in t),
        ),
    )

    pairs, pair_key = pair_runs(base_rows, treat_rows)
    pair_summary = None
    if pairs:
        pair_summary = {
            "n": len(pairs),
            "by": pair_key,
            "conv90_impr_mean": statistics.mean(
                pct_reduction(b["conv90"], t["conv90"]) for b, t in pairs
            ),
            "conv_abs_impr_mean": statistics.mean(
                pct_reduction(b["conv_abs"], t["conv_abs"]) for b, t in pairs
            ),
            "disagree_red_mean": statistics.mean(
                pct_reduction(b["disagreement_rate"], t["disagreement_rate"]) for b, t in pairs
            ),
            "coh_delta_mean": statistics.mean(
                pct_change(t["final_coherence"], b["final_coherence"]) for b, t in pairs
            ),
            "rew_delta_mean": statistics.mean(
                pct_change(t["mean_reward_per_tick"], b["mean_reward_per_tick"]) for b, t in pairs
            ),
            "reward_noninferior_share": statistics.mean(
                1.0 if t["mean_reward_per_tick"] >= b["mean_reward_per_tick"] else 0.0
                for b, t in pairs
            ),
            "coherence_noninferior_share": statistics.mean(
                1.0 if t["final_coherence"] >= b["final_coherence"] else 0.0
                for b, t in pairs
            ),
        }
        pair_summary["conv90_impr_ci"] = bootstrap_ci_paired(
            pairs,
            lambda ps: statistics.mean(pct_reduction(b["conv90"], t["conv90"]) for b, t in ps),
        )
        pair_summary["conv_abs_impr_ci"] = bootstrap_ci_paired(
            pairs,
            lambda ps: statistics.mean(pct_reduction(b["conv_abs"], t["conv_abs"]) for b, t in ps),
        )
        pair_summary["disagree_red_ci"] = bootstrap_ci_paired(
            pairs,
            lambda ps: statistics.mean(
                pct_reduction(b["disagreement_rate"], t["disagreement_rate"]) for b, t in ps
            ),
        )
        pair_summary["coh_delta_ci"] = bootstrap_ci_paired(
            pairs,
            lambda ps: statistics.mean(
                pct_change(t["final_coherence"], b["final_coherence"]) for b, t in ps
            ),
        )
        pair_summary["rew_delta_ci"] = bootstrap_ci_paired(
            pairs,
            lambda ps: statistics.mean(
                pct_change(t["mean_reward_per_tick"], b["mean_reward_per_tick"]) for b, t in ps
            ),
        )

    print("Baseline summary:", bs)
    print("Treatment summary:", ts)
    print("\nA/B deltas (treatment vs baseline):")
    print(f"  conv90_median_improvement: {conv_impr:.2f}%  (95% CI {c_conv[1]:.2f}..{c_conv[2]:.2f})")
    print(f"  conv_abs_improvement@baseline_median_coh({abs_target:.2f}): {abs_conv_impr:.2f}%  (95% CI {c_abs_conv[1]:.2f}..{c_abs_conv[2]:.2f})")
    print(f"  disagreement_reduction:    {dis_red:.2f}%  (95% CI {c_dis[1]:.2f}..{c_dis[2]:.2f})")
    print(f"  final_coherence_delta:     {coh_delta:.2f}%  (95% CI {c_coh[1]:.2f}..{c_coh[2]:.2f})")
    print(f"  reward_delta:              {rew_delta:.2f}%  (95% CI {c_rew[1]:.2f}..{c_rew[2]:.2f})")
    if pair_summary:
        print(f"\nPaired analysis ({pair_summary['n']} pairs by {pair_summary['by']}):")
        print(
            f"  conv90_improvement_mean:   {pair_summary['conv90_impr_mean']:.2f}%  "
            f"(95% CI {pair_summary['conv90_impr_ci'][1]:.2f}..{pair_summary['conv90_impr_ci'][2]:.2f})"
        )
        print(
            f"  conv_abs_improvement_mean: {pair_summary['conv_abs_impr_mean']:.2f}%  "
            f"(95% CI {pair_summary['conv_abs_impr_ci'][1]:.2f}..{pair_summary['conv_abs_impr_ci'][2]:.2f})"
        )
        print(
            f"  disagreement_red_mean:     {pair_summary['disagree_red_mean']:.2f}%  "
            f"(95% CI {pair_summary['disagree_red_ci'][1]:.2f}..{pair_summary['disagree_red_ci'][2]:.2f})"
        )
        print(
            f"  final_coherence_delta_mean:{pair_summary['coh_delta_mean']:.2f}%  "
            f"(95% CI {pair_summary['coh_delta_ci'][1]:.2f}..{pair_summary['coh_delta_ci'][2]:.2f})"
        )
        print(
            f"  reward_delta_mean:         {pair_summary['rew_delta_mean']:.2f}%  "
            f"(95% CI {pair_summary['rew_delta_ci'][1]:.2f}..{pair_summary['rew_delta_ci'][2]:.2f})"
        )
        print(f"  reward_noninferior_share:  {pair_summary['reward_noninferior_share'] * 100.0:.1f}%")
        print(f"  coherence_noninferior_share:{pair_summary['coherence_noninferior_share'] * 100.0:.1f}%")


if __name__ == "__main__":
    main()
