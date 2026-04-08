"""
YC-bench × HSM marketplace runner for NeoSigma auto-harness.

Wire gating.py to:

    benchmark_backend: yc_hsm   # in experiment_config.yaml

After `uv run yc-bench run`, reads either:
  - workspace/yc_hsm_results.json (pre-written), or
  - yc-bench's results/yc_bench_result_<config>_<seed>_<modelslug>.json (rollout → flat scores).

experiment_config.yaml: yc_bench_root, uv_bin, yc_model, yc_seed, yc_config, yc_no_live, yc_extra_args, yc_split_cli.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Dict, List, Optional

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

    def val_score(self, results: dict[str, float]) -> float:
        """Mean reward excluding synthetic `_` keys (e.g. _run_terminal)."""
        filtered = {k: v for k, v in results.items() if not str(k).startswith("_")}
        if not filtered:
            return 0.0
        return sum(filtered.values()) / len(filtered)

    def _results_path(self) -> Path:
        env_p = os.environ.get("YCHSM_RESULTS_JSON")
        if env_p:
            return Path(env_p)
        return (Path.cwd() / DEFAULT_RESULTS).resolve()

    def _rollout_result_path(self) -> Path:
        slug = self.model.replace("/", "_")
        return self.root / "results" / f"yc_bench_result_{self.hsm_config}_{self.seed}_{slug}.json"

    def _build_cmd(self, task_ids: Optional[List[str]]) -> List[str]:
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
        if task_ids:
            flag = os.environ.get("YCHSM_TASK_FLAG", "--task-ids")
            cmd.append(flag)
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

        cmd = self._build_cmd(task_ids)
        print(f"[YcHsmBenchRunner] cwd={self.root} split={self.split}\n  {' '.join(cmd)}", flush=True)

        env = os.environ.copy()
        env.setdefault("PYTHONUNBUFFERED", "1")
        proc = subprocess.run(
            cmd,
            cwd=str(self.root),
            env=env,
            capture_output=True,
            text=True,
        )
        sys.stdout.write(proc.stdout)
        sys.stderr.write(proc.stderr)

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
            subset = {k: full[k] for k in task_ids if k in full}
            missing = [k for k in task_ids if k not in full]
            if missing:
                print(f"[YcHsmBenchRunner] suite keys not in results (showing up to 8): {missing[:8]}", flush=True)
            return subset if subset else {"_no_matching_suite_keys": 0.0}
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
