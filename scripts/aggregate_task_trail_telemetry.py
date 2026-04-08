#!/usr/bin/env python3
import json
import sys
from pathlib import Path


def main() -> int:
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("memory/task_trail.jsonl")
    if not path.exists():
        print(json.dumps({"error": f"missing {path}"}))
        return 1

    turns = []
    with path.open() as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            row = json.loads(line)
            if row.get("kind") == "turn":
                turns.append(row)

    if not turns:
        print(
            json.dumps(
                {
                    "path": str(path),
                    "turns": 0,
                    "avg_tool_prompt_tokens": 0.0,
                    "avg_skill_prompt_tokens": 0.0,
                    "avg_exposed_tools": 0.0,
                    "avg_hidden_tools": 0.0,
                },
                indent=2,
            )
        )
        return 0

    def avg(key: str) -> float:
        return sum(float(r.get(key, 0)) for r in turns) / len(turns)

    print(
        json.dumps(
            {
                "path": str(path),
                "turns": len(turns),
                "avg_tool_prompt_tokens": avg("tool_prompt_tokens"),
                "avg_skill_prompt_tokens": avg("skill_prompt_tokens"),
                "avg_exposed_tools": avg("tool_prompt_exposed_count"),
                "avg_hidden_tools": avg("tool_prompt_hidden_count"),
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

