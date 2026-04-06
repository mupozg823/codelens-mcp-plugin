#!/usr/bin/env python3
"""SPENCER-style layer compression: MiniLM-L12 → L3.

Prunes the student model from 12 layers to 3 layers, then distills
from the full model (teacher) to the pruned model (student).

Expected: ~70% inference speedup, ~2.2% MRR drop (SPENCER Table 3).

Usage:
    python compress_to_3layer.py --teacher scripts/finetune/output/distill-v3-spencer/model
"""

import argparse
import copy
import json
from pathlib import Path

import torch
from sentence_transformers import SentenceTransformer, InputExample, losses
from torch.utils.data import DataLoader

SCRIPT_DIR = Path(__file__).parent
DEFAULT_TEACHER = SCRIPT_DIR / "output" / "v6-internet" / "model"
DEFAULT_DATA = SCRIPT_DIR / "csn_runtime_format.jsonl"
DEFAULT_OUTPUT = SCRIPT_DIR / "output" / "compressed-3layer"


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--teacher", default=str(DEFAULT_TEACHER))
    parser.add_argument("--data", default=str(DEFAULT_DATA))
    parser.add_argument("--output", default=str(DEFAULT_OUTPUT))
    parser.add_argument("--target-layers", type=int, default=3)
    parser.add_argument("--epochs", type=int, default=3)
    parser.add_argument("--batch-size", type=int, default=16)
    parser.add_argument("--max-pairs", type=int, default=10000)
    return parser.parse_args()


def prune_layers(model, target_layers):
    """Remove transformer layers, keeping only the first `target_layers`."""
    auto_model = model[0].auto_model

    # MiniLM-L12 has 12 encoder layers
    encoder = auto_model.encoder
    original_layers = len(encoder.layer)
    if target_layers >= original_layers:
        print(f"Model already has {original_layers} layers, no pruning needed")
        return model

    # Keep only the first N layers + the last layer (for better representations)
    # SPENCER: evenly spaced selection works best
    step = original_layers / target_layers
    keep_indices = [int(i * step) for i in range(target_layers)]
    keep_indices[-1] = original_layers - 1  # always keep last layer

    print(
        f"Pruning {original_layers} → {target_layers} layers (keeping indices: {keep_indices})"
    )

    new_layers = torch.nn.ModuleList(
        [copy.deepcopy(encoder.layer[i]) for i in keep_indices]
    )
    encoder.layer = new_layers

    # Update config
    auto_model.config.num_hidden_layers = target_layers

    return model


def distill_from_teacher(student, teacher, data_path, args):
    """SPENCER dual-modality distillation: CosineSimilarityLoss only."""
    print(f"\n=== Distilling teacher → {args.target_layers}-layer student ===")

    pairs = []
    with open(data_path) as f:
        for line in f:
            obj = json.loads(line.strip())
            if obj.get("positive"):
                pairs.append((obj["query"], obj["positive"]))
            if len(pairs) >= args.max_pairs:
                break

    print(f"  Loaded {len(pairs)} pairs")

    # Generate teacher embeddings for MSE alignment
    print("  Generating teacher embeddings...")
    teacher_queries = [p[0] for p in pairs[:2000]]
    teacher_embs = teacher.encode(
        teacher_queries, show_progress_bar=True, batch_size=32
    )

    # Stage A: MSE alignment (single-modality)
    print("  Stage A: MSE alignment with teacher...")
    device = torch.device("mps" if torch.backends.mps.is_available() else "cpu")
    student_auto = student[0].auto_model.to(device)
    student_tokenizer = student.tokenizer
    target_tensor = torch.tensor(teacher_embs, dtype=torch.float32).to(device)
    optimizer = torch.optim.AdamW(student_auto.parameters(), lr=2e-5)
    mse_loss = torch.nn.MSELoss()

    for epoch in range(2):
        total_loss = 0.0
        batches = 0
        for i in range(0, len(teacher_queries), args.batch_size):
            batch = teacher_queries[i : i + args.batch_size]
            targets = target_tensor[i : i + args.batch_size]
            inputs = student_tokenizer(
                batch,
                padding=True,
                truncation=True,
                max_length=512,
                return_tensors="pt",
            ).to(device)
            outputs = student_auto(**inputs)
            mask = inputs["attention_mask"].unsqueeze(-1).float()
            embs = (outputs.last_hidden_state * mask).sum(1) / mask.sum(1).clamp(
                min=1e-9
            )
            embs = torch.nn.functional.normalize(embs, p=2, dim=1)
            loss = mse_loss(embs, targets)
            loss.backward()
            optimizer.step()
            optimizer.zero_grad()
            total_loss += loss.item()
            batches += 1
        print(f"    Epoch {epoch + 1}/2: MSE = {total_loss / max(batches, 1):.6f}")

    student[0].auto_model = student_auto.cpu()

    # Stage B: MNRL fine-tuning (NOT CosineSimilarityLoss — proven to destroy MRR)
    print("  Stage B: MNRL fine-tuning...")
    examples = [InputExample(texts=[q, p]) for q, p in pairs]
    dataloader = DataLoader(examples, shuffle=True, batch_size=args.batch_size)
    loss_fn = losses.MultipleNegativesRankingLoss(model=student)

    model_output = Path(args.output) / "model"
    model_output.mkdir(parents=True, exist_ok=True)

    student.fit(
        train_objectives=[(dataloader, loss_fn)],
        epochs=args.epochs,
        warmup_steps=int(len(dataloader) * args.epochs * 0.1),
        output_path=str(model_output),
        optimizer_params={"lr": 1e-5},
        show_progress_bar=True,
    )
    print(f"  Compressed model saved to {model_output}")
    return model_output


def export_onnx(model_path, output_dir):
    from optimum.onnxruntime import ORTModelForFeatureExtraction
    from transformers import AutoTokenizer

    onnx_dir = Path(output_dir) / "onnx"
    onnx_dir.mkdir(parents=True, exist_ok=True)
    model = ORTModelForFeatureExtraction.from_pretrained(str(model_path), export=True)
    tokenizer = AutoTokenizer.from_pretrained(str(model_path))
    model.save_pretrained(str(onnx_dir))
    tokenizer.save_pretrained(str(onnx_dir))
    print(f"ONNX exported: {onnx_dir}/model.onnx")
    return onnx_dir


def main():
    args = parse_args()

    print(f"Teacher: {args.teacher}")
    print(f"Target layers: {args.target_layers}")

    teacher = SentenceTransformer(args.teacher)
    student = SentenceTransformer(args.teacher)  # start from same weights
    student = prune_layers(student, args.target_layers)

    model_path = distill_from_teacher(student, teacher, args.data, args)
    export_onnx(model_path, args.output)

    print(f"\nBenchmark:")
    print(
        f"  CODELENS_MODEL_DIR={args.output} python3 benchmarks/embedding-quality.py ."
    )


if __name__ == "__main__":
    main()
