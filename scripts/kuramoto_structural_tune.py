#!/usr/bin/env python3
import argparse
import statistics
import subprocess
from pathlib import Path

from kuramoto_protocol_eval import (
    first_tick_at_or_above,
    load_run_metrics,
    pair_runs,
    pct_change,
    pct_reduction,
    summarize,
)


def run_batch(repo: Path, out_dir: str, cfg: dict, seed_base: int, runs: int, ticks: int):
    cmd = [
        "cargo",
        "run",
        "--release",
        "--bin",
        "batch_experiment",
        "--",
        "--seed-base",
        str(seed_base),
        "--no-credit",
        "--kuramoto",
        "--kuramoto-k",
        str(cfg["k"]),
        "--kuramoto-council",
        str(cfg["council"]),
        "--kuramoto-dt",
        str(cfg["dt"]),
        "--kuramoto-noise",
        str(cfg["noise"]),
        "--kuramoto-gain",
        str(cfg["gain"]),
        "--kuramoto-warmup",
        str(cfg["warmup"]),
        "--kuramoto-cap-k",
        str(cfg["cap_k"]),
        "--kuramoto-cap-c",
        str(cfg["cap_c"]),
        "--kuramoto-lcc-gate",
        str(cfg["lcc_gate"]),
        "--kuramoto-gain-min",
        str(cfg["gain_min"]),
        "--kuramoto-entropy-floor",
        str(cfg["entropy_floor"]),
        "--kuramoto-entropy-boost",
        str(cfg["entropy_boost"]),
        "--kuramoto-disable-trips",
        str(cfg["disable_trips"]),
        str(runs),
        str(ticks),
        out_dir,
    ]
    subprocess.run(cmd, cwd=repo, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def evaluate(base_rows, cand_rows):
    bs = summarize(base_rows)
    abs_target = bs["final_coh_median"]
    for r in base_rows:
        r["conv_abs"] = first_tick_at_or_above(r, abs_target)
    for r in cand_rows:
        r["conv_abs"] = first_tick_at_or_above(r, abs_target)

    pairs, _ = pair_runs(base_rows, cand_rows)
    if not pairs:
        return None
    return {
        "conv_abs": statistics.mean(pct_reduction(b["conv_abs"], t["conv_abs"]) for b, t in pairs),
        "dis": statistics.mean(
            pct_reduction(b["disagreement_rate"], t["disagreement_rate"]) for b, t in pairs
        ),
        "coh": statistics.mean(pct_change(t["final_coherence"], b["final_coherence"]) for b, t in pairs),
        "rew": statistics.mean(
            pct_change(t["mean_reward_per_tick"], b["mean_reward_per_tick"]) for b, t in pairs
        ),
    }


def passes(metrics):
    return (
        metrics["conv_abs"] >= 0.0
        and metrics["dis"] >= 0.0
        and metrics["coh"] >= 0.0
        and metrics["rew"] >= 0.0
    )


def main():
    p = argparse.ArgumentParser(description="Coarse->fine structural Kuramoto tuning under non-inferiority.")
    p.add_argument("--baseline", required=True)
    p.add_argument("--seed-base", type=int, default=2026022401)
    p.add_argument("--runs", type=int, default=20)
    p.add_argument("--ticks", type=int, default=1000)
    p.add_argument("--prefix", default="experiments_kura_struct_tune")
    p.add_argument("--topk", type=int, default=3)
    args = p.parse_args()

    repo = Path(__file__).resolve().parents[1]
    base_rows = load_run_metrics(repo / args.baseline)
    if not base_rows:
        raise SystemExit("Baseline not found or empty.")

    coarse = [
        dict(k=0.04, council=0.008, dt=0.0005, noise=0.01, gain=0.02, warmup=400, cap_k=0.04, cap_c=0.018, lcc_gate=0.85, gain_min=0.10, entropy_floor=0.45, entropy_boost=0.03, disable_trips=5),
        dict(k=0.05, council=0.01, dt=0.0005, noise=0.01, gain=0.03, warmup=400, cap_k=0.05, cap_c=0.02, lcc_gate=0.85, gain_min=0.10, entropy_floor=0.45, entropy_boost=0.03, disable_trips=5),
        dict(k=0.05, council=0.01, dt=0.0005, noise=0.01, gain=0.04, warmup=400, cap_k=0.05, cap_c=0.02, lcc_gate=0.85, gain_min=0.10, entropy_floor=0.45, entropy_boost=0.03, disable_trips=5),
        dict(k=0.06, council=0.012, dt=0.0005, noise=0.01, gain=0.03, warmup=300, cap_k=0.06, cap_c=0.022, lcc_gate=0.85, gain_min=0.10, entropy_floor=0.45, entropy_boost=0.03, disable_trips=5),
    ]

    evaluated = []
    for i, cfg in enumerate(coarse, start=1):
        out = f"{args.prefix}_coarse_{i}"
        run_batch(repo, out, cfg, args.seed_base, args.runs, args.ticks)
        metrics = evaluate(base_rows, load_run_metrics(repo / out))
        if metrics is None:
            continue
        evaluated.append((out, cfg, metrics))

    survivors = [x for x in evaluated if passes(x[2])]
    survivors.sort(key=lambda x: (x[2]["conv_abs"], x[2]["coh"], x[2]["rew"]), reverse=True)
    seeds = survivors[: args.topk] if survivors else evaluated[: args.topk]

    fine = []
    for _, cfg, _ in seeds:
        for dk, dc, dg in [(-0.01, -0.002, -0.01), (0.0, 0.0, 0.0), (0.01, 0.002, 0.01)]:
            fine.append(
                dict(
                    cfg,
                    k=max(0.01, cfg["k"] + dk),
                    council=max(0.001, cfg["council"] + dc),
                    gain=max(0.0, cfg["gain"] + dg),
                )
            )

    fine_eval = []
    for i, cfg in enumerate(fine, start=1):
        out = f"{args.prefix}_fine_{i}"
        run_batch(repo, out, cfg, args.seed_base, args.runs, args.ticks)
        metrics = evaluate(base_rows, load_run_metrics(repo / out))
        if metrics is None:
            continue
        fine_eval.append((out, cfg, metrics))

    all_eval = evaluated + fine_eval
    passing = [x for x in all_eval if passes(x[2])]
    passing.sort(key=lambda x: (x[2]["conv_abs"], x[2]["coh"], x[2]["rew"]), reverse=True)

    print(f"Total evaluated: {len(all_eval)}")
    print(f"Passing: {len(passing)}")
    if passing:
        best = passing[0]
        print("Best:", best[0], best[2], best[1])
    else:
        print("No passing config found.")


if __name__ == "__main__":
    main()
