# Patch for auto-harness gating.py (replace TauBenchRunner block in `if __name__ == "__main__"`).
#
# from benchmark import BenchmarkRunner, TauBenchRunner
from benchmark import BenchmarkRunner
from ychsm_benchmark_runner import YcHsmBenchRunner

# ...

if __name__ == "__main__":
    cfg = load_config()
    # Keep domain check satisfied by experiment_config.yaml placeholder domain.
    if "domain" not in cfg:
        print("ERROR: 'domain' not set in experiment_config.yaml")
        sys.exit(1)

    train_runner = YcHsmBenchRunner(split=cfg.get("split", "train"))
    gate_runner = YcHsmBenchRunner(split=cfg.get("gate_split", "test"))
    sys.exit(run_gate(train_runner, gate_runner))
