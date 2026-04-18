#!/usr/bin/env python3
"""
check_company_drift.py — detect changes in installed Paperclip company packs.

Mirrors the logic of skills-lock.json / skills drift detection, but for the
competitor company packs under ~/.hsm/company-packs/paperclipai/companies/.

Usage:
    python3 scripts/check_company_drift.py              # check drift, exit 1 if any
    python3 scripts/check_company_drift.py --update     # rewrite companies-lock.json

Exit codes:
    0  — no drift detected
    1  — one or more companies drifted (details printed to stdout)
    2  — lock file missing (run with --update to create it)
"""

import argparse
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
LOCK_FILE = REPO_ROOT / "companies-lock.json"
PACKS_ROOT = Path.home() / ".hsm" / "company-packs" / "paperclipai" / "companies"


def hash_file(p: Path) -> str:
    h = hashlib.sha256()
    h.update(p.read_bytes())
    return h.hexdigest()


def pack_summary(company_dir: Path) -> dict:
    """Compute hashes for all canonical pack files (excludes .recursive/ runtime state)."""
    files = {}
    for f in sorted(company_dir.rglob("*")):
        if f.is_file() and ".recursive" not in f.parts:
            rel = str(f.relative_to(company_dir))
            files[rel] = hash_file(f)
    combined = hashlib.sha256(json.dumps(files, sort_keys=True).encode()).hexdigest()
    return {
        "source": f"paperclipai/companies/{company_dir.name}",
        "sourceType": "local_pack",
        "contentHash": combined,
        "agents": len(list(company_dir.glob("agents/*/AGENTS.md"))),
        "skills": len(list(company_dir.glob("skills/*/SKILL.md"))),
        "fileHashes": files,
    }


def load_lock() -> dict:
    if not LOCK_FILE.exists():
        return {}
    return json.loads(LOCK_FILE.read_text())


def write_lock(companies: dict) -> None:
    lock = {
        "version": 1,
        "locked_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "pack_root": "~/.hsm/company-packs/paperclipai/companies",
        "companies": companies,
    }
    LOCK_FILE.write_text(json.dumps(lock, indent=2))
    print(f"Wrote {LOCK_FILE} ({len(companies)} companies)")


def check(update: bool) -> int:
    if not PACKS_ROOT.exists():
        print(f"ERROR: pack root not found: {PACKS_ROOT}", file=sys.stderr)
        return 2

    current = {
        c.name: pack_summary(c)
        for c in sorted(PACKS_ROOT.iterdir())
        if c.is_dir()
    }

    if update:
        write_lock(current)
        return 0

    lock = load_lock()
    if not lock:
        print("ERROR: companies-lock.json not found. Run with --update to create it.")
        return 2

    locked = lock.get("companies", {})
    locked_at = lock.get("locked_at", "unknown")

    drifted = []
    added = []
    removed = []

    for name, data in current.items():
        if name not in locked:
            added.append(name)
        elif data["contentHash"] != locked[name]["contentHash"]:
            # Find which files changed
            old_files = locked[name].get("fileHashes", {})
            new_files = data["fileHashes"]
            changed = [
                f for f in set(old_files) | set(new_files)
                if old_files.get(f) != new_files.get(f)
            ]
            drifted.append((name, changed))

    for name in locked:
        if name not in current:
            removed.append(name)

    if not drifted and not added and not removed:
        print(f"OK — all {len(current)} company packs match lock ({locked_at})")
        return 0

    print(f"DRIFT DETECTED (locked at {locked_at})\n")

    if drifted:
        print(f"Changed ({len(drifted)}):")
        for name, files in drifted:
            old_skills = locked[name]["skills"]
            new_skills = current[name]["skills"]
            old_agents = locked[name]["agents"]
            new_agents = current[name]["agents"]
            skill_delta = f" skills {old_skills}→{new_skills}" if old_skills != new_skills else ""
            agent_delta = f" agents {old_agents}→{new_agents}" if old_agents != new_agents else ""
            print(f"  {name}{skill_delta}{agent_delta}")
            for f in sorted(files)[:10]:
                print(f"    ~ {f}")
            if len(files) > 10:
                print(f"    ... and {len(files) - 10} more")

    if added:
        print(f"\nNew packs ({len(added)}):")
        for name in added:
            print(f"  + {name}  ({current[name]['agents']} agents, {current[name]['skills']} skills)")

    if removed:
        print(f"\nRemoved packs ({len(removed)}):")
        for name in removed:
            print(f"  - {name}")

    print("\nRun `python3 scripts/check_company_drift.py --update` to accept changes.")
    print("Consider re-running affected yc-bench benchmarks:")
    for name, _ in drifted:
        print(f"  cargo run --bin hsm_outer_loop -- external-batch --spec config/external_yc_bench_seed7.json  # filter: {name}")

    return 1


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--update", action="store_true", help="Rewrite companies-lock.json from current packs")
    args = parser.parse_args()
    sys.exit(check(args.update))


if __name__ == "__main__":
    main()
