#!/usr/bin/env python3
"""V8 Final: LoRA MNRL with LLM synthetic + CSN + feedback data.

Trains TWO models in one run:
  A) LoRA on V6 (preserve domain adaptation)
  B) LoRA on Base all-MiniLM-L12-v2 (fresh NL capabilities)

Picks the winner by eval score.

Usage:
    python3 scripts/finetune/train_v8_final.py
    python3 scripts/finetune/train_v8_final.py --dry-run
"""

from __future__ import annotations

import argparse
import json
import os
import random
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
ROOT = SCRIPT_DIR.parent.parent
V6_MODEL = SCRIPT_DIR / "output" / "v6-internet" / "model"
BASE_MODEL = "sentence-transformers/all-MiniLM-L12-v2"
INPUT = SCRIPT_DIR / "v9_rust_final_training.jsonl"
OUTPUT_DIR = SCRIPT_DIR / "output" / "v9-rust-final"
MAX_SEQ_LENGTH = 128
BENCHMARK = ROOT / "benchmarks" / "embedding-quality-dataset.json"


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=Path, default=INPUT)
    parser.add_argument("--output-dir", type=Path, default=OUTPUT_DIR)
    parser.add_argument("--epochs", type=int, default=3)
    parser.add_argument("--micro-batch", type=int, default=32)
    parser.add_argument("--effective-batch", type=int, default=128)
    parser.add_argument("--lr", type=float, default=2e-4)
    parser.add_argument("--lora-rank", type=int, default=16)
    parser.add_argument("--eval-split", type=float, default=0.1)
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--dry-run", action="store_true")
    return parser.parse_args()


def load_pairs(path: Path) -> list[dict]:
    pairs = []
    with open(path) as f:
        for line in f:
            if line.strip():
                obj = json.loads(line)
                q = obj.get("query", "").strip()
                p = obj.get("positive", "").strip()
                if q and p and len(q.split()) >= 2:
                    pairs.append({"query": q, "positive": p})
    return pairs


def train_lora(model_name_or_path, train_pairs, eval_pairs, output_path, args, label):
    import torch
    from peft import LoraConfig, get_peft_model, TaskType
    from sentence_transformers import SentenceTransformer, InputExample, losses
    from torch.utils.data import DataLoader

    print(f"\n{'='*50}")
    print(f"  Training: {label}")
    print(f"  Base: {model_name_or_path}")
    print(f"  Train: {len(train_pairs)}, Eval: {len(eval_pairs)}")
    print(f"{'='*50}")

    device = (
        "mps"
        if torch.backends.mps.is_available()
        else "cuda" if torch.cuda.is_available() else "cpu"
    )
    print(f"  Device: {device}")

    model = SentenceTransformer(str(model_name_or_path))
    model.max_seq_length = MAX_SEQ_LENGTH

    lora_config = LoraConfig(
        task_type=TaskType.FEATURE_EXTRACTION,
        r=args.lora_rank,
        lora_alpha=args.lora_rank * 2,
        lora_dropout=0.1,
        target_modules=["query", "value"],
    )

    auto_model = model[0].auto_model
    peft_model = get_peft_model(auto_model, lora_config)
    trainable = sum(p.numel() for p in peft_model.parameters() if p.requires_grad)
    total = sum(p.numel() for p in peft_model.parameters())
    print(f"  LoRA: {trainable:,} / {total:,} ({trainable/total*100:.2f}%)")
    model[0].auto_model = peft_model

    examples = [InputExample(texts=[p["query"], p["positive"]]) for p in train_pairs]
    loader = DataLoader(examples, shuffle=True, batch_size=args.micro_batch)
    loss_fn = losses.MultipleNegativesRankingLoss(model=model)

    warmup = int(len(loader) * args.epochs * 0.1)
    model_out = Path(output_path) / "model"
    model_out.mkdir(parents=True, exist_ok=True)

    best_score = -1.0
    best_epoch = 0

    for epoch in range(args.epochs):
        print(f"\n  --- Epoch {epoch+1}/{args.epochs} ---")
        model.fit(
            train_objectives=[(loader, loss_fn)],
            epochs=1,
            warmup_steps=warmup if epoch == 0 else 0,
            optimizer_params={"lr": args.lr},
            show_progress_bar=True,
            output_path=str(model_out),
        )

        score = quick_eval(model, eval_pairs)
        print(f"  Eval: {score:.4f}")

        if score > best_score:
            best_score = score
            best_epoch = epoch + 1
            # Merge and save
            merged = peft_model.merge_and_unload()
            model[0].auto_model = merged
            model.save(str(model_out))
            print(f"  Best! Saved (score={best_score:.4f})")
            # Re-wrap for next epoch
            peft_model = get_peft_model(merged, lora_config)
            model[0].auto_model = peft_model
        else:
            print(f"  No improvement")
            if epoch - best_epoch >= 1:
                print(f"  Early stopping")
                break

    # Clean up adapter files if they leaked
    for f in (
        model_out / "adapter_config.json",
        model_out / "adapter_model.safetensors",
    ):
        if f.exists():
            f.unlink()

    print(f"  Final: epoch {best_epoch}, score {best_score:.4f}")
    return model_out, best_score


def quick_eval(model, eval_pairs):
    import torch

    qs = [p["query"] for p in eval_pairs[:500]]
    ps = [p["positive"] for p in eval_pairs[:500]]
    with torch.no_grad():
        qe = model.encode(
            qs, batch_size=64, show_progress_bar=False, convert_to_tensor=True
        )
        pe = model.encode(
            ps, batch_size=64, show_progress_bar=False, convert_to_tensor=True
        )
    qn = torch.nn.functional.normalize(qe, p=2, dim=1)
    pn = torch.nn.functional.normalize(pe, p=2, dim=1)
    return (qn * pn).sum(dim=1).mean().item()


def benchmark_model(model_path, label):
    """Run full benchmark comparison."""
    import torch
    import re
    from sentence_transformers import SentenceTransformer

    with open(BENCHMARK) as f:
        dataset = json.load(f)

    nl = [q for q in dataset if q["query_type"] == "natural_language"]

    def split_id(name):
        if "_" in name:
            parts = [p for p in name.split("_") if p]
            exp = []
            for p in parts:
                s = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", p)
                s = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1 \2", s)
                exp.extend(s.split())
            return " ".join(w.lower() for w in exp if w)
        s = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", name)
        s = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1 \2", s)
        return " ".join(w.lower() for w in s.split() if w)

    def build_pos(q):
        n = q["expected_symbol"]
        sp = split_id(n)
        ns = f"{n} ({sp})" if sp != n.lower() else n
        return f"function {ns} in {q['expected_file_suffix']}"

    model = SentenceTransformer(str(model_path))
    model.max_seq_length = MAX_SEQ_LENGTH

    qtexts = [q["query"] for q in nl]
    ptexts = [build_pos(q) for q in nl]

    with torch.no_grad():
        qe = model.encode(
            qtexts, batch_size=64, convert_to_tensor=True, show_progress_bar=False
        )
        pe = model.encode(
            ptexts, batch_size=64, convert_to_tensor=True, show_progress_bar=False
        )

    qn = torch.nn.functional.normalize(qe, p=2, dim=1)
    pn = torch.nn.functional.normalize(pe, p=2, dim=1)
    sim = qn @ pn.T

    ranks = []
    for i in range(len(qtexts)):
        r = (sim[i] > sim[i][i]).sum().item() + 1
        ranks.append(r)

    mrr = sum(1 / r for r in ranks) / len(ranks)
    a1 = sum(1 for r in ranks if r == 1) / len(ranks)
    a3 = sum(1 for r in ranks if r <= 3) / len(ranks)
    fails = sum(1 for r in ranks if r > 1)

    print(f"\n  {label} Benchmark:")
    print(f"    NL MRR:  {mrr:.4f}")
    print(f"    Acc@1:   {a1:.1%}")
    print(f"    Acc@3:   {a3:.1%}")
    print(f"    Failures: {fails}/{len(ranks)}")
    return mrr


def export_onnx(model_path, output_dir):
    from optimum.onnxruntime import ORTModelForFeatureExtraction
    from transformers import AutoTokenizer

    onnx_dir = Path(output_dir) / "onnx"
    onnx_dir.mkdir(parents=True, exist_ok=True)
    model = ORTModelForFeatureExtraction.from_pretrained(str(model_path), export=True)
    tokenizer = AutoTokenizer.from_pretrained(str(model_path))
    model.save_pretrained(str(onnx_dir))
    tokenizer.save_pretrained(str(onnx_dir))

    try:
        from onnxruntime.quantization import quantize_dynamic, QuantType

        fp32 = str(onnx_dir / "model.onnx")
        int8 = str(onnx_dir / "model_qint8.onnx")
        quantize_dynamic(fp32, int8, weight_type=QuantType.QInt8)
        print(f"  ONNX INT8: {os.path.getsize(int8)/1024/1024:.1f}MB")
    except Exception as e:
        print(f"  Quantization skipped: {e}")

    return onnx_dir


def main():
    args = parse_args()
    random.seed(args.seed)

    print("=" * 60)
    print("V8 Final Training — LLM Synthetic + CSN + Feedback")
    print("=" * 60)

    pairs = load_pairs(args.input)
    print(f"Loaded {len(pairs)} valid pairs")

    random.shuffle(pairs)
    split = int(len(pairs) * (1 - args.eval_split))
    train = pairs[:split]
    val = pairs[split:]
    print(f"Train: {len(train)}, Eval: {len(val)}")

    if args.dry_run:
        print("[DRY RUN] Done.")
        return

    # Train A: LoRA on V6
    out_a = args.output_dir / "model-a-v6"
    path_a, score_a = train_lora(V6_MODEL, train, val, out_a, args, "A: LoRA on V6")

    # Train B: LoRA on Base
    out_b = args.output_dir / "model-b-base"
    path_b, score_b = train_lora(BASE_MODEL, train, val, out_b, args, "B: LoRA on Base")

    # Benchmark both + V6 baseline
    print("\n" + "=" * 60)
    print("  BENCHMARK COMPARISON")
    print("=" * 60)
    mrr_v6 = benchmark_model(V6_MODEL, "V6 (baseline)")
    mrr_a = benchmark_model(path_a, "V8-A (LoRA on V6)")
    mrr_b = benchmark_model(path_b, "V8-B (LoRA on Base)")

    # Pick winner
    results = [
        ("V6 baseline", mrr_v6, None),
        ("V8-A", mrr_a, path_a),
        ("V8-B", mrr_b, path_b),
    ]
    results.sort(key=lambda x: x[1], reverse=True)
    winner_name, winner_mrr, winner_path = results[0]

    print(f"\n{'='*60}")
    print(f"  WINNER: {winner_name} (MRR={winner_mrr:.4f})")
    print(f"{'='*60}")

    if winner_path:
        onnx = export_onnx(winner_path, args.output_dir)
        print(f"\nONNX exported: {onnx}")
        print(f"Deploy: cp -r {onnx}/* models/codelens-code-search/arm64/")
    else:
        print("\nV6 baseline won — no new model to deploy.")


if __name__ == "__main__":
    main()
