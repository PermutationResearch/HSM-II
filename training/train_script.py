#!/usr/bin/env python3
"""
Contrastive training script (sentence-pair cosine similarity + cross-entropy loss).
Designed to scale to large datasets and emit reward reports for HSM-II.
"""
import json
import os
import random
from dataclasses import dataclass
from pathlib import Path
from typing import List, Dict

import torch
import torch.nn.functional as F
from torch.utils.data import Dataset, DataLoader

try:
    from transformers import AutoModel, AutoTokenizer
except Exception as exc:
    raise SystemExit(f"transformers required: {exc}")


@dataclass
class TrainConfig:
    model_name: str = "sentence-transformers/all-MiniLM-L6-v2"
    max_length: int = 128
    batch_size: int = 1024
    steps: int = 100_000
    lr: float = 2e-5
    warmup_steps: int = 500
    weight_decay: float = 0.01
    output_dir: str = "training/checkpoints"
    manifest_path: str = "training/data/manifest.jsonl"
    reward_log_path: str = "training/reward_reports.jsonl"
    valid_manifest_path: str = ""
    valid_batches: int = 20
    eval_every: int = 1000
    seed: int = 42


class PairDataset(Dataset):
    def __init__(self, manifest_path: str):
        self.items: List[Dict] = []
        with open(manifest_path, "r", encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                self.items.append(json.loads(line))

    def __len__(self):
        return len(self.items)

    def __getitem__(self, idx: int):
        item = self.items[idx]
        return item["text_a"], item["text_b"], int(item.get("label", 1))


def seed_all(seed: int):
    random.seed(seed)
    torch.manual_seed(seed)
    torch.cuda.manual_seed_all(seed)


def cosine_sim(a: torch.Tensor, b: torch.Tensor) -> torch.Tensor:
    a = F.normalize(a, p=2, dim=-1)
    b = F.normalize(b, p=2, dim=-1)
    return a @ b.t()


def make_dataloader(cfg: TrainConfig, tokenizer):
    dataset = PairDataset(cfg.manifest_path)

    def collate(batch):
        texts_a, texts_b, labels = zip(*batch)
        a = tokenizer(list(texts_a), padding=True, truncation=True, max_length=cfg.max_length, return_tensors="pt")
        b = tokenizer(list(texts_b), padding=True, truncation=True, max_length=cfg.max_length, return_tensors="pt")
        return a, b, torch.tensor(labels, dtype=torch.long)

    return DataLoader(dataset, batch_size=cfg.batch_size, shuffle=True, collate_fn=collate, drop_last=True)


def make_valid_dataloader(cfg: TrainConfig, tokenizer):
    if cfg.valid_manifest_path:
        dataset = PairDataset(cfg.valid_manifest_path)
    else:
        dataset = PairDataset(cfg.manifest_path)

    def collate(batch):
        texts_a, texts_b, labels = zip(*batch)
        a = tokenizer(list(texts_a), padding=True, truncation=True, max_length=cfg.max_length, return_tensors="pt")
        b = tokenizer(list(texts_b), padding=True, truncation=True, max_length=cfg.max_length, return_tensors="pt")
        return a, b, torch.tensor(labels, dtype=torch.long)

    return DataLoader(dataset, batch_size=cfg.batch_size, shuffle=False, collate_fn=collate, drop_last=True)


def info_nce_loss(sim: torch.Tensor, labels: torch.Tensor) -> torch.Tensor:
    # labels: 1 for positive pair, 0 for negative.
    # treat each row as query; positive is diagonal when labels==1 (assumed positive pairs are aligned).
    target = torch.arange(sim.size(0), device=sim.device)
    return F.cross_entropy(sim, target)


def main():
    cfg = TrainConfig(
        model_name=os.environ.get("HSM_MODEL", "sentence-transformers/all-MiniLM-L6-v2"),
        manifest_path=os.environ.get("HSM_MANIFEST", "training/data/manifest.jsonl"),
        reward_log_path=os.environ.get("HSM_REWARD_LOG", "training/reward_reports.jsonl"),
        valid_manifest_path=os.environ.get("HSM_VALID_MANIFEST", ""),
    )
    cfg.steps = int(os.environ.get("HSM_STEPS", cfg.steps))
    cfg.eval_every = int(os.environ.get("HSM_EVAL_EVERY", cfg.eval_every))
    cfg.valid_batches = int(os.environ.get("HSM_VALID_BATCHES", cfg.valid_batches))

    seed_all(cfg.seed)
    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")

    tokenizer = AutoTokenizer.from_pretrained(cfg.model_name)
    model = AutoModel.from_pretrained(cfg.model_name).to(device)
    model.train()

    optimizer = torch.optim.AdamW(model.parameters(), lr=cfg.lr, weight_decay=cfg.weight_decay)

    dataloader = make_dataloader(cfg, tokenizer)
    valid_loader = make_valid_dataloader(cfg, tokenizer)
    total_steps = cfg.steps

    def lr_lambda(step: int):
        if step < cfg.warmup_steps:
            return float(step) / float(max(1, cfg.warmup_steps))
        return max(0.0, (total_steps - step) / float(max(1, total_steps - cfg.warmup_steps)))

    scheduler = torch.optim.lr_scheduler.LambdaLR(optimizer, lr_lambda)

    Path(cfg.output_dir).mkdir(parents=True, exist_ok=True)
    reward_log = Path(cfg.reward_log_path)
    reward_log.parent.mkdir(parents=True, exist_ok=True)

    def run_validation(tag: str):
        model.eval()
        losses = []
        accs = []
        with torch.no_grad():
            for i, batch in enumerate(valid_loader):
                if i >= cfg.valid_batches:
                    break
                batch_a, batch_b, labels = batch
                batch_a = {k: v.to(device) for k, v in batch_a.items()}
                batch_b = {k: v.to(device) for k, v in batch_b.items()}
                out_a = model(**batch_a).last_hidden_state[:, 0]
                out_b = model(**batch_b).last_hidden_state[:, 0]
                sim = cosine_sim(out_a, out_b)
                loss = info_nce_loss(sim, labels)
                preds = sim.argmax(dim=1)
                acc = (preds == torch.arange(sim.size(0), device=sim.device)).float().mean().item()
                losses.append(float(loss.item()))
                accs.append(acc)
        model.train()
        mean_loss = sum(losses) / max(1, len(losses))
        mean_acc = sum(accs) / max(1, len(accs))
        print(f"[valid:{tag}] loss={mean_loss:.4f} acc={mean_acc:.4f} batches={len(losses)}")
        return mean_loss, mean_acc

    step = 0
    data_iter = iter(dataloader)

    # Baseline validation
    base_loss, base_acc = run_validation("baseline")
    while step < cfg.steps:
        try:
            batch = next(data_iter)
        except StopIteration:
            data_iter = iter(dataloader)
            batch = next(data_iter)

        batch_a, batch_b, labels = batch
        batch_a = {k: v.to(device) for k, v in batch_a.items()}
        batch_b = {k: v.to(device) for k, v in batch_b.items()}

        out_a = model(**batch_a).last_hidden_state[:, 0]
        out_b = model(**batch_b).last_hidden_state[:, 0]

        sim = cosine_sim(out_a, out_b)
        loss = info_nce_loss(sim, labels)

        optimizer.zero_grad(set_to_none=True)
        loss.backward()
        optimizer.step()
        scheduler.step()

        # Compute accuracy: diag similarity highest?
        with torch.no_grad():
            preds = sim.argmax(dim=1)
            accuracy = (preds == torch.arange(sim.size(0), device=sim.device)).float().mean().item()

        # Emit reward report (task_score = accuracy, ground_truth_score = 1 - loss (scaled))
        report = {
            "coherence_delta": 0.0,
            "exec_ok": True,
            "task_score": accuracy,
            "ground_truth_score": max(0.0, 1.0 - float(loss.item())),
            "tests_passed": None,
            "latency_penalty": None,
        }
        with reward_log.open("a", encoding="utf-8") as f:
            f.write(json.dumps(report) + "\n")

        if step % 100 == 0:
            print(f"step={step} loss={loss.item():.4f} acc={accuracy:.4f}")
        if cfg.eval_every > 0 and step % cfg.eval_every == 0 and step > 0:
            run_validation(f"step_{step}")
        if step % 1000 == 0 and step > 0:
            ckpt = Path(cfg.output_dir) / f"ckpt_{step}.pt"
            torch.save({"model": model.state_dict(), "step": step}, ckpt)

        step += 1

    # Final validation
    final_loss, final_acc = run_validation("final")
    summary = {
        "baseline_loss": base_loss,
        "baseline_acc": base_acc,
        "final_loss": final_loss,
        "final_acc": final_acc,
        "steps": cfg.steps,
    }
    summary_path = Path(cfg.output_dir) / "signal_test_summary.json"
    summary_path.write_text(json.dumps(summary, indent=2))
    print(f"Saved validation summary: {summary_path}")


if __name__ == "__main__":
    main()
