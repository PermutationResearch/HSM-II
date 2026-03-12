#!/usr/bin/env python3
"""
Build a sharded JSONL manifest for contrastive training.
Supports streaming JSONL/TSV sources with simple field mappings.
"""
import argparse
import json
from pathlib import Path
from typing import Iterator, Dict


def iter_jsonl(path: Path):
    with path.open("r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            yield json.loads(line)


def iter_tsv(path: Path, text_a_col: int, text_b_col: int, label_col: int = None):
    with path.open("r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            parts = line.split("\t")
            if len(parts) <= max(text_a_col, text_b_col, label_col or 0):
                continue
            item = {
                "text_a": parts[text_a_col],
                "text_b": parts[text_b_col],
                "label": int(parts[label_col]) if label_col is not None else 1,
            }
            yield item


def load_source(src: Dict) -> Iterator[Dict]:
    path = Path(src["path"])
    src_type = src.get("type", "jsonl")
    if src_type == "jsonl":
        for item in iter_jsonl(path):
            yield {
                "text_a": item[src.get("text_a", "text_a")],
                "text_b": item[src.get("text_b", "text_b")],
                "label": int(item.get(src.get("label", "label"), 1)),
            }
    elif src_type == "tsv":
        yield from iter_tsv(path, src.get("text_a_col", 0), src.get("text_b_col", 1), src.get("label_col"))
    else:
        raise ValueError(f"Unsupported source type: {src_type}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", required=True, help="Path to dataset config JSON")
    parser.add_argument("--output", required=True, help="Output directory for shards")
    parser.add_argument("--shard-size", type=int, default=5_000_000, help="Lines per shard")
    parser.add_argument("--limit", type=int, default=None, help="Max total records")
    args = parser.parse_args()

    config = json.loads(Path(args.config).read_text())
    out_dir = Path(args.output)
    out_dir.mkdir(parents=True, exist_ok=True)

    shard_idx = 0
    shard_count = 0
    total = 0

    def open_shard(idx: int):
        return (out_dir / f"manifest_{idx:04}.jsonl").open("w", encoding="utf-8")

    shard_file = open_shard(shard_idx)

    for src in config.get("sources", []):
        for item in load_source(src):
            shard_file.write(json.dumps(item) + "\n")
            shard_count += 1
            total += 1
            if args.limit and total >= args.limit:
                break
            if shard_count >= args.shard_size:
                shard_file.close()
                shard_idx += 1
                shard_file = open_shard(shard_idx)
                shard_count = 0
        if args.limit and total >= args.limit:
            break

    shard_file.close()
    print(f"Wrote {total} records across {shard_idx + 1} shards to {out_dir}")


if __name__ == "__main__":
    main()
