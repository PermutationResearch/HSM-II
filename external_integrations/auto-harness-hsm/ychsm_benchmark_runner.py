"""
YC-bench × HSM marketplace runner for NeoSigma auto-harness.

Wire gating.py to:

    benchmark_backend: yc_hsm   # in experiment_config.yaml

After `uv run yc-bench run`, reads either:
  - workspace/yc_hsm_results.json (pre-written), or
  - yc-bench's results/yc_bench_result_<config>_<seed>_<modelslug>.json (rollout → flat scores).

experiment_config.yaml: yc_bench_root, uv_bin, yc_model, yc_seed, yc_config, yc_no_live, yc_extra_args, yc_split_cli,
  yc_rollout_mode (single | aggregate_existing).
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path
from typing import Dict, List, Optional, Tuple

import yaml

from benchmark import BenchmarkRunner

CONFIG_FILE = "experiment_config.yaml"
WORKSPACE = Path("workspace")
DEFAULT_RESULTS = WORKSPACE / "yc_hsm_results.json"


def _load_cfg() -> dict:
    if not Path(CONFIG_FILE).exists():
        return {}
    with open(CONFIG_FILE) as f:
        return yaml.safe_load(f) or {}


def from_yc_bench_rollout(doc: dict) -> Dict[str, float]:
    """Map yc-bench saved rollout JSON to flat task_id -> reward (same as HSM normalize script)."""
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


class YcHsmBenchRunner(BenchmarkRunner):
    """Runs yc-bench as a subprocess and loads per-task rewards from JSON."""

    def __init__(self, split: str = "train"):
        self.cfg = _load_cfg()
        self.split = split
        self.root = Path(self.cfg.get("yc_bench_root", os.environ.get("YC_BENCH_ROOT", "."))).resolve()
        self.uv_bin = self.cfg.get("uv_bin", os.environ.get("YC_BENCH_UV", "uv"))
        self.model = self.cfg.get("yc_model", os.environ.get("YC_BENCH_MODEL", "openrouter/qwen/qwen3.6-plus:free"))
        self.seed = int(self.cfg.get("yc_seed", os.environ.get("YC_BENCH_SEED", "6")))
        self.hsm_config = self.cfg.get("yc_config", os.environ.get("YC_BENCH_HSM_CONFIG", "hsm_market_apex-systems"))
        self.extra: List[str] = list(self.cfg.get("yc_extra_args", []))
        self.no_live = bool(self.cfg.get("yc_no_live", True))
        self.split_args: dict = dict(self.cfg.get("yc_split_cli", {}))
        self.rollout_mode = str(self.cfg.get("yc_rollout_mode", "single")).strip().lower()
        self._pending_aggregate_val_score: Optional[float] = None
        self.bench_max_retries = int(self.cfg.get("yc_bench_max_retries", 3))
        self.bench_retry_base_s = float(self.cfg.get("yc_bench_retry_base_seconds", 30))

    @staticmethod
    def _yc_bench_output_suggests_retry(combined: str) -> bool:
        """Heuristic: provider rate limits / overload (OpenRouter free tier is noisy)."""
        s = combined.lower()
        needles = (
            "429",
            "rate limit",
            "ratelimit",
            "too many requests",
            "resource exhausted",
            "over capacity",
            "temporarily unavailable",
            "503",
            "502",
            "connection reset",
            "timed out",
            "timeout",
        )
        return any(n in s for n in needles)

    def val_score(self, results: dict[str, float]) -> float:
        """Mean reward excluding synthetic `_` keys (e.g. _run_terminal)."""
        if self._pending_aggregate_val_score is not None:
            v = self._pending_aggregate_val_score
            self._pending_aggregate_val_score = None
            return v
        filtered = {k: v for k, v in results.items() if not str(k).startswith("_")}
        if not filtered:
            return 0.0
        return sum(filtered.values()) / len(filtered)

    def _results_path(self) -> Path:
        env_p = os.environ.get("YCHSM_RESULTS_JSON")
        if env_p:
            return Path(env_p)
        return (Path.cwd() / DEFAULT_RESULTS).resolve()

    def _model_slug(self) -> str:
        return self.model.replace("/", "_")

    def _rollout_result_path(self) -> Path:
        slug = self._model_slug()
        return self.root / "results" / f"yc_bench_result_{self.hsm_config}_{self.seed}_{slug}.json"

    def _rollout_suffix_matches_model(self, file_suffix: str) -> bool:
        """yc-bench writes `<seed>_<modelslug>.json`; slug may use `_` instead of `:`."""
        slug = self._model_slug()
        if file_suffix == slug:
            return True
        if file_suffix == slug.replace(":", "_"):
            return True
        return False

    def _discover_rollout_paths(self) -> List[Tuple[int, Path]]:
        """Paths under yc_bench_root/results for this config + model slug (seed parsed from filename)."""
        results_dir = self.root / "results"
        if not results_dir.is_dir():
            return []
        prefix = f"yc_bench_result_{self.hsm_config}_"
        pat = re.compile("^" + re.escape(prefix) + r"(\d+)_(.+)\.json$")
        out: List[Tuple[int, Path]] = []
        for p in results_dir.iterdir():
            if not p.is_file():
                continue
            m = pat.match(p.name)
            if not m:
                continue
            seed_s, rest = m.group(1), m.group(2)
            if not self._rollout_suffix_matches_model(rest):
                continue
            out.append((int(seed_s), p))
        out.sort(key=lambda x: x[0])
        return out

    def _merge_rollout_flats(self, flats: List[Dict[str, float]]) -> Dict[str, float]:
        """Per-key mean across seeds (union of keys)."""
        if not flats:
            return {}
        keys: set[str] = set()
        for f in flats:
            keys.update(f.keys())
        merged: Dict[str, float] = {}
        for k in keys:
            vals = [float(f[k]) for f in flats if k in f]
            if vals:
                merged[k] = sum(vals) / len(vals)
        return merged

    def _load_rollout_flats_from_disk(self) -> Tuple[List[Dict[str, float]], List[int]]:
        flats: List[Dict[str, float]] = []
        seeds_seen: List[int] = []
        for seed, path in self._discover_rollout_paths():
            try:
                with open(path) as f:
                    doc = json.load(f)
            except (OSError, json.JSONDecodeError) as e:
                print(f"[YcHsmBenchRunner] skip unreadable rollout {path}: {e}", flush=True)
                continue
            if not isinstance(doc, dict):
                continue
            flat = from_yc_bench_rollout(doc)
            if not flat:
                continue
            flats.append(flat)
            seeds_seen.append(seed)
        return flats, seeds_seen

    def _aggregate_merged_flat(self) -> Optional[Dict[str, float]]:
        """Merged per-turn scores from all matching rollouts; no val_score side effects."""
        flats, _ = self._load_rollout_flats_from_disk()
        if not flats:
            return None
        return self._merge_rollout_flats(flats)

    def _try_load_aggregate_rollouts(self) -> Optional[Dict[str, float]]:
        flats, seeds_seen = self._load_rollout_flats_from_disk()
        if not flats:
            return None
        per_seed_vals = []
        for f in flats:
            filtered = {k: v for k, v in f.items() if not str(k).startswith("_")}
            if filtered:
                per_seed_vals.append(sum(filtered.values()) / len(filtered))
        mean_vs = sum(per_seed_vals) / len(per_seed_vals) if per_seed_vals else 0.0
        self._pending_aggregate_val_score = mean_vs
        merged = self._merge_rollout_flats(flats)
        print(
            f"[YcHsmBenchRunner] aggregate_existing: seeds {seeds_seen} "
            f"(n={len(flats)}), mean val_score={mean_vs:.4f}",
            flush=True,
        )
        return merged

    @staticmethod
    def _pick_task_subset(full: Dict[str, float], task_ids: List[str]) -> Dict[str, float]:
        """Every requested id is present; missing keys score 0.0 (stable pass-rate denominator)."""
        out: Dict[str, float] = {}
        missing: List[str] = []
        for k in task_ids:
            if k in full:
                out[k] = full[k]
            else:
                missing.append(k)
                out[k] = 0.0
        if missing:
            print(
                f"[YcHsmBenchRunner] {len(missing)} task id(s) not in results (using 0.0), "
                f"sample: {missing[:8]}",
                flush=True,
            )
        return out

    def _build_cmd(self, task_ids: Optional[List[str]] = None) -> List[str]:
        """Full rollout. Optional `YCHSM_TASK_FLAG` + value for a forked yc-bench that supports task filters."""
        cmd: List[str] = [
            self.uv_bin,
            "run",
            "yc-bench",
            "run",
            "--model",
            self.model,
            "--seed",
            str(self.seed),
            "--config",
            self.hsm_config,
        ]
        if self.no_live:
            cmd.append("--no-live")
        cmd.extend(self.extra)
        split_key = self.split
        extra_split = self.split_args.get(split_key)
        if isinstance(extra_split, list):
            cmd.extend(str(x) for x in extra_split)
        opt_flag = (os.environ.get("YCHSM_TASK_FLAG") or "").strip()
        if task_ids and opt_flag:
            cmd.append(opt_flag)
            cmd.append(",".join(task_ids))
        return cmd

    def _parse_flat_file(self, path: Path) -> Dict[str, float]:
        with open(path) as f:
            raw = json.load(f)
        if isinstance(raw, dict) and "results" in raw and isinstance(raw["results"], dict):
            raw = raw["results"]
        out: Dict[str, float] = {}
        if not isinstance(raw, dict):
            return out
        for k, v in raw.items():
            try:
                out[str(k)] = float(v)
            except (TypeError, ValueError):
                continue
        return out

    def _materialize_from_rollout(self, dest: Path) -> bool:
        rp = self._rollout_result_path()
        if not rp.is_file():
            return False
        with open(rp) as f:
            doc = json.load(f)
        flat = from_yc_bench_rollout(doc) if isinstance(doc, dict) else {}
        if not flat:
            return False
        dest.parent.mkdir(parents=True, exist_ok=True)
        with open(dest, "w") as f:
            json.dump(flat, f, indent=2, sort_keys=True)
            f.write("\n")
        print(f"[YcHsmBenchRunner] wrote {dest} from rollout {rp} ({len(flat)} keys)", flush=True)
        return True

    def run(self, task_ids: Optional[List[str]] = None) -> Dict[str, float]:
        WORKSPACE.mkdir(parents=True, exist_ok=True)
        results_path = self._results_path()
        if results_path.exists():
            results_path.unlink()

        if task_ids and self.rollout_mode == "aggregate_existing":
            merged = self._aggregate_merged_flat()
            if merged is not None:
                picked = self._pick_task_subset(merged, task_ids)
                results_path.parent.mkdir(parents=True, exist_ok=True)
                with open(results_path, "w") as f:
                    json.dump(picked, f, indent=2, sort_keys=True)
                    f.write("\n")
                print(
                    f"[YcHsmBenchRunner] aggregate_existing: subset {len(task_ids)} task(s) "
                    f"from merged rollouts (no yc-bench run)",
                    flush=True,
                )
                return picked
            print(
                "[YcHsmBenchRunner] aggregate_existing: no rollouts for subset — "
                "running full yc-bench then slicing tasks",
                flush=True,
            )

        if task_ids is None and self.rollout_mode == "aggregate_existing":
            agg = self._try_load_aggregate_rollouts()
            if agg:
                results_path.parent.mkdir(parents=True, exist_ok=True)
                with open(results_path, "w") as f:
                    json.dump(agg, f, indent=2, sort_keys=True)
                    f.write("\n")
                print(f"[YcHsmBenchRunner] wrote {results_path} from aggregate rollouts", flush=True)
                return agg
            print(
                "[YcHsmBenchRunner] aggregate_existing: no matching rollouts — "
                f"falling back to yc-bench run (seed={self.seed})",
                flush=True,
            )

        cmd = self._build_cmd(task_ids)
        if task_ids and not (os.environ.get("YCHSM_TASK_FLAG") or "").strip():
            print(
                "[YcHsmBenchRunner] note: upstream yc-bench has no task filter; running full rollout, "
                f"then selecting {len(task_ids)} task(s). Set YCHSM_TASK_FLAG if your fork adds one.",
                flush=True,
            )
        env = os.environ.copy()
        env.setdefault("PYTHONUNBUFFERED", "1")

        proc: Optional[subprocess.CompletedProcess[str]] = None
        attempts = max(0, self.bench_max_retries) + 1
        for attempt in range(attempts):
            print(
                f"[YcHsmBenchRunner] cwd={self.root} split={self.split} "
                f"(attempt {attempt + 1}/{attempts})\n  {' '.join(cmd)}",
                flush=True,
            )
            proc = subprocess.run(
                cmd,
                cwd=str(self.root),
                env=env,
                capture_output=True,
                text=True,
            )
            sys.stdout.write(proc.stdout)
            sys.stderr.write(proc.stderr)

            if proc.returncode == 0:
                break

            combined = (proc.stdout or "") + "\n" + (proc.stderr or "")
            retryable = self._yc_bench_output_suggests_retry(combined)
            last_attempt = attempt + 1 >= attempts
            if not retryable or last_attempt:
                print(
                    f"[YcHsmBenchRunner] yc-bench exited {proc.returncode}",
                    file=sys.stderr,
                )
                return {"_bench_failed": 0.0}

            delay = self.bench_retry_base_s * (2**attempt)
            print(
                f"[YcHsmBenchRunner] transient failure — sleeping {delay:.0f}s then retry "
                f"({attempt + 1}/{attempts - 1} retries left)",
                flush=True,
            )
            time.sleep(delay)

        assert proc is not None
        if proc.returncode != 0:
            print(f"[YcHsmBenchRunner] yc-bench exited {proc.returncode}", file=sys.stderr)
            return {"_bench_failed": 0.0}

        out_path = self._results_path()
        if not out_path.exists():
            self._materialize_from_rollout(out_path)

        if not out_path.exists():
            print(
                "[YcHsmBenchRunner] Missing results. Expected "
                f"{out_path} or yc-bench rollout at {self._rollout_result_path()}",
                file=sys.stderr,
            )
            return {"_missing_results_json": 0.0}

        full = self._parse_flat_file(out_path)
        if not full:
            return {"_empty_results": 0.0}

        if task_ids:
            picked = self._pick_task_subset(full, task_ids)
            return picked if picked else {"_no_matching_suite_keys": 0.0}
        return full


if __name__ == "__main__":
    import argparse
    import datetime

    parser = argparse.ArgumentParser()
    parser.add_argument("--split", default="train")
    parser.add_argument("--task-ids", nargs="*", default=None)
    args = parser.parse_args()
    r = YcHsmBenchRunner(split=args.split)
    res = r.run(task_ids=args.task_ids)
    print(f"\nval_score: {r.val_score(res):.4f} ({len(res)} tasks)")
    for tid, rv in sorted(res.items(), key=lambda x: (str(x[0]).startswith("_"), x[0])):
        st = "PASS" if rv >= 0.5 else "FAIL"
        print(f"  {st} {tid}: {rv:.2f}")

    WORKSPACE.mkdir(parents=True, exist_ok=True)
    dump_path = WORKSPACE / "train_results.json"
    with open(dump_path, "w") as f:
        json.dump(
            {
                "split": args.split,
                "timestamp": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
                "results": res,
            },
            f,
            indent=2,
        )
    print(f"[YcHsmBenchRunner] wrote {dump_path}")
