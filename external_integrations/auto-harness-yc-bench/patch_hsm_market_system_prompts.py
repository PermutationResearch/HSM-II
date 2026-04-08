#!/usr/bin/env python3
"""
Append YC-bench CLI mechanics to every hsm_market_*.toml [agent] system_prompt.

Idempotent: skips files that already contain YC_BENCH_SIM_CLI_V1.

Usage:
  python3 patch_hsm_market_system_prompts.py /path/to/yc-bench
  YC_BENCH_ROOT=~/yc-bench python3 patch_hsm_market_system_prompts.py
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

MARKER = "YC_BENCH_SIM_CLI_V1"
SNIPPET_FILE = Path(__file__).resolve().parent / "snippets" / "yc_bench_sim_cli_rules.txt"


def patch_content(toml: str, snippet: str) -> tuple[str, bool]:
    if MARKER in toml:
        return toml, False
    key = 'system_prompt = """'
    i = toml.find(key)
    if i < 0:
        return toml, False
    start = i + len(key)
    rest = toml[start:]
    j = rest.rfind('"""')
    if j < 0:
        return toml, False
    insert = "\n\n" + snippet.strip() + "\n"
    new_toml = toml[:start] + rest[:j] + insert + rest[j:]
    return new_toml, True


def main() -> int:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else os.environ.get("YC_BENCH_ROOT", "")).resolve()
    preset_dir = root / "src" / "yc_bench" / "config" / "presets"
    if not preset_dir.is_dir():
        print(f"ERROR: presets dir not found: {preset_dir}", file=sys.stderr)
        return 1
    snippet = SNIPPET_FILE.read_text()
    if MARKER not in snippet:
        print("ERROR: snippet missing marker", file=sys.stderr)
        return 1

    paths = sorted(preset_dir.glob("hsm_market_*.toml"))
    if not paths:
        print(f"ERROR: no hsm_market_*.toml under {preset_dir}", file=sys.stderr)
        return 1

    n = 0
    for p in paths:
        raw = p.read_text()
        new, changed = patch_content(raw, snippet)
        if changed:
            p.write_text(new)
            print(f"Patched {p.name}")
            n += 1
        else:
            print(f"Skip  {p.name} (already patched or no system_prompt)")

    print(f"Done. Updated {n}/{len(paths)} file(s).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
