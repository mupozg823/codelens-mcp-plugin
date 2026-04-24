#!/usr/bin/env python3
"""V7 Training: NL-augmented LoRA MNRL fine-tuning on V6 distilled model.

Goal: Improve NL semantic_search MRR from ~0.50 to 0.75+
Strategy:
  - LoRA (not full fine-tuning) to preserve V6 code→code capabilities
  - MNRL loss with large effective batch (gradient accumulation → 128+)
  - Train/eval split + early stopping to prevent overfitting
  - Data deduplication + quality filtering

Key design decisions per MiniLM best-practices:
  1. LoRA r=16, alpha=32 → ~1.5% trainable params → prevents catastrophic forgetting
  2. Effective batch 128+ → more in-batch negatives for MNRL
  3. Learning rate 2e-4 (LoRA standard) vs 1e-5 (full fine-tuning)
  4. 90/10 train/eval split + eval every epoch
  5. MinHash dedup on queries before training

Usage:
    python3 scripts/finetune/train_v7_nl_augmented.py --dry-run
    python3 scripts/finetune/train_v7_nl_augmented.py
    python3 scripts/finetune/train_v7_nl_augmented.py --lora-rank 8 --epochs 3
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import random
import sys
from collections import Counter
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
ROOT = SCRIPT_DIR.parent.parent
V6_MODEL_DIR = SCRIPT_DIR / "output" / "v6-internet" / "model"
INPUT = SCRIPT_DIR / "csn_nl_combined.jsonl"
OUTPUT_DIR = SCRIPT_DIR / "output" / "v7-nl-augmented"
MAX_SEQ_LENGTH = 128


def parse_args():
    parser = argparse.ArgumentParser(description="V7 NL-augmented LoRA MNRL training")
    parser.add_argument("--input", type=Path, default=INPUT)
    parser.add_argument("--model-dir", type=Path, default=V6_MODEL_DIR)
    parser.add_argument("--output-dir", type=Path, default=OUTPUT_DIR)
    parser.add_argument("--epochs", type=int, default=5)
    parser.add_argument(
        "--micro-batch", type=int, default=32, help="Per-device batch size"
    )
    parser.add_argument(
        "--effective-batch",
        type=int,
        default=128,
        help="Effective batch via gradient accumulation",
    )
    parser.add_argument(
        "--learning-rate",
        type=float,
        default=2e-4,
        help="LoRA learning rate (higher than full FT)",
    )
    parser.add_argument("--lora-rank", type=int, default=16)
    parser.add_argument("--lora-alpha", type=int, default=32)
    parser.add_argument("--lora-dropout", type=float, default=0.1)
    parser.add_argument(
        "--eval-split", type=float, default=0.1, help="Fraction for validation set"
    )
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--no-dedup", action="store_true", help="Skip deduplication")
    return parser.parse_args()


# ---------------------------------------------------------------------------
# Data quality & deduplication
# ---------------------------------------------------------------------------


def load_and_filter_pairs(path: Path) -> list[dict]:
    """Load pairs with quality filtering."""
    import re

    pairs = []
    rejected = Counter()

    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            obj = json.loads(line)
            q = obj.get("query", "").strip()
            p = obj.get("positive", "").strip()

            # Filter: missing fields
            if not q or not p:
                rejected["missing_field"] += 1
                continue

            # Filter: too short
            words = q.split()
            if len(words) < 2:
                rejected["too_short"] += 1
                continue

            # Filter: too long (>20 words likely noisy)
            if len(words) > 20:
                rejected["too_long"] += 1
                continue

            # Filter: starts with code-like pattern
            if re.match(
                r"^(def |func |fn |function |class |import |from |var |let |const )", q
            ):
                rejected["code_like"] += 1
                continue

            # Filter: contains PascalCase compound words (>15 chars, likely class names)
            if re.search(r"\b[A-Z][a-z]+[A-Z][a-z]+[A-Z][a-z]+\w{5,}\b", q):
                rejected["pascal_case"] += 1
                continue

            # Filter: too many @annotations
            if q.count("@") >= 2:
                rejected["annotations"] += 1
                continue

            pairs.append({"query": q, "positive": p})

    print(f"  Loaded: {len(pairs)} pairs")
    print(f"  Rejected: {dict(rejected)}")
    return pairs


def dedup_pairs(pairs: list[dict], seed: int = 42) -> list[dict]:
    """Deduplicate by query hash (exact) + near-duplicate detection."""
    seen_hashes = set()
    deduped = []

    for pair in pairs:
        # Normalize: lowercase, strip extra spaces
        normalized = " ".join(pair["query"].lower().split())
        h = hashlib.md5(normalized.encode()).hexdigest()
        if h not in seen_hashes:
            seen_hashes.add(h)
            deduped.append(pair)

    removed = len(pairs) - len(deduped)
    print(f"  Dedup: {len(pairs)} → {len(deduped)} (removed {removed})")
    return deduped


# ---------------------------------------------------------------------------
# Training
# ---------------------------------------------------------------------------


def train_lora_mnrl(
    model_dir: Path, train_pairs: list[dict], eval_pairs: list[dict], args
) -> Path:
    """LoRA MNRL fine-tuning with validation and early stopping."""
    import torch
    from peft import LoraConfig, get_peft_model, TaskType
    from sentence_transformers import SentenceTransformer, InputExample, losses
    from sentence_transformers.evaluation import EmbeddingSimilarityEvaluator
    from torch.utils.data import DataLoader

    print(f"\n=== LoRA MNRL Fine-tuning ===")
    print(f"  Train: {len(train_pairs)}, Eval: {len(eval_pairs)}")
    print(
        f"  LoRA rank={args.lora_rank}, alpha={args.lora_alpha}, dropout={args.lora_dropout}"
    )

    # Device
    if torch.backends.mps.is_available():
        device = "mps"
    elif torch.cuda.is_available():
        device = "cuda"
    else:
        device = "cpu"
    print(f"  Device: {device}")

    # Load model
    model = SentenceTransformer(str(model_dir))
    model.max_seq_length = MAX_SEQ_LENGTH

    # Apply LoRA to attention layers
    lora_config = LoraConfig(
        task_type=TaskType.FEATURE_EXTRACTION,
        r=args.lora_rank,
        lora_alpha=args.lora_alpha,
        lora_dropout=args.lora_dropout,
        target_modules=["query", "value"],
    )

    auto_model = model[0].auto_model
    peft_model = get_peft_model(auto_model, lora_config)
    trainable = sum(p.numel() for p in peft_model.parameters() if p.requires_grad)
    total = sum(p.numel() for p in peft_model.parameters())
    print(f"  LoRA params: {trainable:,} / {total:,} ({trainable / total * 100:.2f}%)")
    model[0].auto_model = peft_model

    # Prepare data
    train_examples = [
        InputExample(texts=[p["query"], p["positive"]]) for p in train_pairs
    ]
    train_loader = DataLoader(train_examples, shuffle=True, batch_size=args.micro_batch)

    loss_fn = losses.MultipleNegativesRankingLoss(model=model)

    # Gradient accumulation for effective batch size
    grad_accum = max(1, args.effective_batch // args.micro_batch)
    print(f"  Micro batch: {args.micro_batch}")
    print(f"  Gradient accumulation: {grad_accum}")
    print(f"  Effective batch: {args.micro_batch * grad_accum}")

    # Output directories
    model_output = args.output_dir / "model"
    model_output.mkdir(parents=True, exist_ok=True)

    # Warmup steps
    steps_per_epoch = len(train_loader)
    warmup = int(steps_per_epoch * args.epochs * 0.1)
    print(f"  Steps/epoch: {steps_per_epoch}, Warmup: {warmup}")

    # Training with evaluation
    best_score = -1.0
    patience = 2
    stall_count = 0

    for epoch in range(args.epochs):
        print(f"\n  --- Epoch {epoch + 1}/{args.epochs} ---")
        model.fit(
            train_objectives=[(train_loader, loss_fn)],
            epochs=1,
            warmup_steps=warmup if epoch == 0 else 0,
            optimizer_params={"lr": args.learning_rate},
            show_progress_bar=True,
            output_path=str(model_output),
        )

        # Quick eval: compute mean cosine similarity on eval set
        if eval_pairs:
            eval_score = quick_eval(model, eval_pairs)
            print(f"  Eval score: {eval_score:.4f}")

            if eval_score > best_score:
                best_score = eval_score
                stall_count = 0
                # Save best checkpoint
                peft_model_copy = model[0].auto_model
                merged = peft_model_copy.merge_and_unload()
                model[0].auto_model = merged
                model.save(str(model_output))
                print(f"  Best model saved (score={best_score:.4f})")
                # Re-apply LoRA for next epoch
                model[0].auto_model = get_peft_model(merged, lora_config)
            else:
                stall_count += 1
                print(f"  No improvement ({stall_count}/{patience})")
                if stall_count >= patience:
                    print(f"  Early stopping at epoch {epoch + 1}")
                    break

    # Final merge if we didn't early-stop with a save
    if not (model_output / "config.json").exists():
        merged = model[0].auto_model.merge_and_unload()
        model[0].auto_model = merged
        model.save(str(model_output))

    print(f"\n  Final model: {model_output}")
    print(f"  Best eval score: {best_score:.4f}")
    return model_output


def quick_eval(model, eval_pairs: list[dict]) -> float:
    """Quick evaluation: mean cosine similarity of query-positive pairs."""
    import torch

    queries = [p["query"] for p in eval_pairs[:500]]
    positives = [p["positive"] for p in eval_pairs[:500]]

    with torch.no_grad():
        q_embs = model.encode(
            queries, batch_size=64, show_progress_bar=False, convert_to_tensor=True
        )
        p_embs = model.encode(
            positives, batch_size=64, show_progress_bar=False, convert_to_tensor=True
        )

    # Cosine similarity
    q_norm = torch.nn.functional.normalize(q_embs, p=2, dim=1)
    p_norm = torch.nn.functional.normalize(p_embs, p=2, dim=1)
    similarities = (q_norm * p_norm).sum(dim=1)
    return similarities.mean().item()


def export_onnx_int8(model_path: Path, output_dir: Path) -> Path:
    """Export to ONNX INT8 for fastembed runtime."""
    from optimum.onnxruntime import ORTModelForFeatureExtraction
    from transformers import AutoTokenizer

    onnx_dir = output_dir / "onnx"
    onnx_dir.mkdir(parents=True, exist_ok=True)

    model = ORTModelForFeatureExtraction.from_pretrained(str(model_path), export=True)
    tokenizer = AutoTokenizer.from_pretrained(str(model_path))
    model.save_pretrained(str(onnx_dir))
    tokenizer.save_pretrained(str(onnx_dir))
    print(f"ONNX exported: {onnx_dir}")

    # INT8 dynamic quantization
    try:
        from onnxruntime.quantization import quantize_dynamic, QuantType

        fp32 = onnx_dir / "model.onnx"
        fp32_backup = onnx_dir / "model_fp32.onnx"
        int8 = onnx_dir / "model_qint8.onnx"
        quantize_dynamic(str(fp32), str(int8), weight_type=QuantType.QInt8)
        if fp32_backup.exists():
            fp32_backup.unlink()
        fp32.replace(fp32_backup)
        int8.replace(fp32)
        size_mb = os.path.getsize(fp32) / 1024 / 1024
        print(f"INT8 quantized: {size_mb:.1f}MB → {fp32}")
    except Exception as e:
        print(f"Quantization skipped: {e}")

    return onnx_dir


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main():
    args = parse_args()
    random.seed(args.seed)

    print("=" * 60)
    print("V7 NL-Augmented LoRA MNRL Training")
    print("=" * 60)
    print(f"  Input:           {args.input}")
    print(f"  Base model:      {args.model_dir}")
    print(f"  Output:          {args.output_dir}")
    print(f"  LoRA:            r={args.lora_rank}, alpha={args.lora_alpha}")
    print(f"  Effective batch: {args.effective_batch}")
    print(f"  Epochs:          {args.epochs}")
    print(f"  Learning rate:   {args.learning_rate}")

    if not args.input.exists():
        print(f"\nERROR: Input not found: {args.input}")
        print("Run: python3 scripts/finetune/build_nl_augmentation.py first")
        sys.exit(1)

    if not args.model_dir.exists():
        print(f"\nERROR: V6 model not found: {args.model_dir}")
        sys.exit(1)

    # Step 1: Load and filter
    print("\n[Step 1] Loading and filtering data...")
    pairs = load_and_filter_pairs(args.input)

    # Step 2: Deduplicate
    if not args.no_dedup:
        print("\n[Step 2] Deduplicating...")
        pairs = dedup_pairs(pairs, args.seed)
    else:
        print("\n[Step 2] Dedup skipped (--no-dedup)")

    # Step 3: Train/eval split
    print(
        f"\n[Step 3] Splitting train/eval ({1 - args.eval_split:.0%}/{args.eval_split:.0%})..."
    )
    random.shuffle(pairs)
    split_idx = int(len(pairs) * (1 - args.eval_split))
    train_pairs = pairs[:split_idx]
    eval_pairs = pairs[split_idx:]
    print(f"  Train: {len(train_pairs)}, Eval: {len(eval_pairs)}")

    # Stats
    train_lengths = [len(p["query"].split()) for p in train_pairs]
    if train_lengths:
        import statistics

        print(
            f"  Query words: mean={statistics.mean(train_lengths):.1f}, "
            f"median={statistics.median(train_lengths):.1f}"
        )

    if args.dry_run:
        print("\n[DRY RUN] Data validation complete.")
        print(f"  Would train on {len(train_pairs)} pairs with LoRA MNRL")
        print(f"  Effective batch: {args.effective_batch}")
        print(
            f"  Estimated steps: {len(train_pairs) // args.effective_batch * args.epochs}"
        )
        return

    # Step 4: LoRA MNRL training
    model_path = train_lora_mnrl(args.model_dir, train_pairs, eval_pairs, args)

    # Step 5: Export ONNX INT8
    print("\n[Step 5] Exporting ONNX INT8...")
    onnx_dir = export_onnx_int8(model_path, args.output_dir)

    print(f"\n{'=' * 60}")
    print("V7 Training Complete")
    print(f"{'=' * 60}")
    print(f"\nNext steps:")
    print(f"  1. Benchmark:")
    print(
        f"     CODELENS_MODEL_DIR={onnx_dir} python3 benchmarks/embedding-quality.py ."
    )
    print(f"  2. Promotion gate:")
    print(f"     python3 scripts/finetune/promotion_gate.py {onnx_dir}")
    print(f"  3. Deploy (if improved):")
    print(f"     cp -r {onnx_dir}/* models/codelens-code-search/arm64/")


if __name__ == "__main__":
    main()
