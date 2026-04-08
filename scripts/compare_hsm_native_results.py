#!/usr/bin/env python3
import json
import sys
from pathlib import Path


def load_rows(path: Path):
    rows = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    out = {}
    for row in rows:
        out.setdefault(row["task_id"], {})[row["variant"]] = row
    return out


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: compare_hsm_native_results.py <tasks.jsonl>")
        return 1
    path = Path(sys.argv[1])
    rows = load_rows(path)
    baseline_only = []
    hsm_only = []
    both_wrong = []
    both_right = []
    for task_id, pair in sorted(rows.items()):
        b = pair.get("baseline")
        h = pair.get("hsm-full")
        if not b or not h:
            continue
        bp = b.get("answer_accuracy", 0.0) >= 1.0
        hp = h.get("answer_accuracy", 0.0) >= 1.0
        if bp and hp:
            both_right.append(task_id)
        elif bp and not hp:
            baseline_only.append(task_id)
        elif hp and not bp:
            hsm_only.append(task_id)
        else:
            both_wrong.append(task_id)
    report = {
        "tasks_file": str(path),
        "baseline_only_wins": baseline_only,
        "hsm_only_wins": hsm_only,
        "both_right": both_right,
        "both_wrong": both_wrong,
        "counts": {
            "baseline_only_wins": len(baseline_only),
            "hsm_only_wins": len(hsm_only),
            "both_right": len(both_right),
            "both_wrong": len(both_wrong),
        },
    }
    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

