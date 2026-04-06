#!/usr/bin/env python3
"""Two-stage fine-tuning: domain adaptation + task-specific.

Stage 1: Adapt all-MiniLM-L12-v2 to code domain using CodeSearchNet pairs
Stage 2: Fine-tune on project-specific triplets

Output: scripts/finetune/output/v2/model/ + output/v2/onnx/
"""

import argparse
import json
import os
import random
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
ROOT = SCRIPT_DIR.parent.parent
DEFAULT_GENERAL_STAGE2 = SCRIPT_DIR / "training_pairs_augmented.jsonl"
DEFAULT_CODEX_STAGE2 = SCRIPT_DIR / "training_pairs_codex.jsonl"


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--profile", choices=["general", "codex"], default="general")
    parser.add_argument(
        "--stage1-pairs",
        type=int,
        default=5000,
        help="Number of CodeSearchNet pairs for domain adaptation",
    )
    parser.add_argument("--stage1-epochs", type=int, default=1)
    parser.add_argument("--stage2-input", default="")
    parser.add_argument("--stage2-epochs", type=int, default=10)
    parser.add_argument("--batch-size", type=int, default=32)
    parser.add_argument("--lr", type=float, default=2e-5)
    parser.add_argument("--output", default="")
    parser.add_argument(
        "--base-model", default="sentence-transformers/all-MiniLM-L12-v2"
    )
    parser.add_argument("--skip-stage1", action="store_true")
    parser.add_argument("--skip-onnx", action="store_true")
    return parser.parse_args()


def resolve_stage2_input(profile: str, explicit_input: str) -> Path:
    if explicit_input:
        return Path(explicit_input)
    if profile == "codex":
        return DEFAULT_CODEX_STAGE2
    return DEFAULT_GENERAL_STAGE2


def resolve_output(profile: str, explicit_output: str) -> Path:
    if explicit_output:
        return Path(explicit_output)
    if profile == "codex":
        return SCRIPT_DIR / "output" / "codex-v2"
    return SCRIPT_DIR / "output" / "v2"


def generate_code_pairs(n=5000):
    """Generate (docstring, code) pairs from the project codebase for domain adaptation."""
    pairs = []
    code_dir = ROOT / "crates"

    for rs_file in code_dir.rglob("*.rs"):
        try:
            content = rs_file.read_text()
        except Exception:
            continue

        lines = content.split("\n")
        i = 0
        while i < len(lines):
            # Find doc comments (///)
            doc_lines = []
            while i < len(lines) and lines[i].strip().startswith("///"):
                doc_text = lines[i].strip().lstrip("/").strip()
                if doc_text:
                    doc_lines.append(doc_text)
                i += 1

            # Find the function/struct signature after doc comments
            if doc_lines and i < len(lines):
                sig_line = lines[i].strip()
                if any(
                    kw in sig_line
                    for kw in ["fn ", "struct ", "enum ", "impl ", "pub "]
                ):
                    doc = " ".join(doc_lines)
                    # Collect up to 5 lines of body
                    body_lines = []
                    for j in range(i, min(i + 5, len(lines))):
                        body_lines.append(lines[j])
                    code = "\n".join(body_lines)

                    if len(doc) > 10 and len(code) > 20:
                        pairs.append((doc, code))
            i += 1

        if len(pairs) >= n:
            break

    random.shuffle(pairs)
    return pairs[:n]


def load_triplets(path):
    triplets = []
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            obj = json.loads(line)
            if obj.get("negative"):
                triplets.append((obj["query"], obj["positive"], obj["negative"]))
    return triplets


def stage1_domain_adaptation(model, pairs, args):
    """Adapt to code domain using (docstring, code) pairs with MultipleNegativesRankingLoss."""
    from sentence_transformers import InputExample, losses
    from torch.utils.data import DataLoader

    print(
        f"\n=== Stage 1: Domain Adaptation ({len(pairs)} pairs, {args.stage1_epochs} epochs) ==="
    )

    examples = [InputExample(texts=[doc, code]) for doc, code in pairs]
    dataloader = DataLoader(examples, shuffle=True, batch_size=args.batch_size)
    loss = losses.MultipleNegativesRankingLoss(model=model)

    warmup = int(len(dataloader) * args.stage1_epochs * 0.1)
    model.fit(
        train_objectives=[(dataloader, loss)],
        epochs=args.stage1_epochs,
        warmup_steps=warmup,
        optimizer_params={"lr": args.lr},
        show_progress_bar=True,
    )
    print("Stage 1 complete.")
    return model


def stage2_task_finetune(model, triplets, args):
    """Fine-tune on project-specific triplets with TripletLoss."""
    from sentence_transformers import InputExample, losses
    from sentence_transformers.evaluation import TripletEvaluator
    from torch.utils.data import DataLoader

    print(
        f"\n=== Stage 2: Task Fine-tuning ({len(triplets)} triplets, {args.stage2_epochs} epochs) ==="
    )

    split = int(len(triplets) * 0.9)
    train_triplets = triplets[:split]
    eval_triplets = triplets[split:]

    train_examples = [InputExample(texts=[q, p, n]) for q, p, n in train_triplets]
    dataloader = DataLoader(train_examples, shuffle=True, batch_size=args.batch_size)
    loss = losses.TripletLoss(model=model)

    evaluator = (
        TripletEvaluator(
            anchors=[t[0] for t in eval_triplets],
            positives=[t[1] for t in eval_triplets],
            negatives=[t[2] for t in eval_triplets],
            name="eval",
        )
        if eval_triplets
        else None
    )

    model_output = Path(args.output) / "model"
    model_output.mkdir(parents=True, exist_ok=True)

    warmup = int(len(dataloader) * args.stage2_epochs * 0.1)
    model.fit(
        train_objectives=[(dataloader, loss)],
        epochs=args.stage2_epochs,
        warmup_steps=warmup,
        evaluator=evaluator,
        evaluation_steps=max(1, len(dataloader) // 2),
        output_path=str(model_output),
        optimizer_params={"lr": args.lr * 0.5},  # Lower LR for stage 2
        show_progress_bar=True,
    )
    print(f"Stage 2 complete. Model saved to {model_output}")
    return model_output


def export_onnx(model_path, output_dir):
    try:
        from optimum.onnxruntime import ORTModelForFeatureExtraction
        from transformers import AutoTokenizer
    except ImportError:
        print("pip install optimum[onnxruntime]")
        return None

    onnx_dir = Path(output_dir) / "onnx"
    onnx_dir.mkdir(parents=True, exist_ok=True)

    print(f"\nExporting to ONNX → {onnx_dir}")
    model = ORTModelForFeatureExtraction.from_pretrained(str(model_path), export=True)
    tokenizer = AutoTokenizer.from_pretrained(str(model_path))
    model.save_pretrained(str(onnx_dir))
    tokenizer.save_pretrained(str(onnx_dir))
    print(f"ONNX model saved: {onnx_dir}/model.onnx")
    return onnx_dir


def main():
    args = parse_args()
    stage2_input = resolve_stage2_input(args.profile, args.stage2_input)
    output_dir = resolve_output(args.profile, args.output)

    try:
        from sentence_transformers import SentenceTransformer
    except ImportError:
        print("pip install sentence-transformers torch datasets accelerate")
        sys.exit(1)

    print(f"Base model: {args.base_model}")
    model = SentenceTransformer(args.base_model)

    # Stage 1: Domain adaptation
    if not args.skip_stage1:
        pairs = generate_code_pairs(args.stage1_pairs)
        print(f"Generated {len(pairs)} (docstring, code) pairs from codebase")
        if pairs:
            model = stage1_domain_adaptation(model, pairs, args)
        else:
            print("No docstring pairs found, skipping stage 1")

    # Stage 2: Task-specific fine-tuning
    if not stage2_input.exists():
        if args.profile == "codex":
            print(f"Codex dataset not found: {stage2_input}")
            print(f"Build it first: python {SCRIPT_DIR}/build_codex_dataset.py")
        else:
            print(f"No triplets in {stage2_input}")
        sys.exit(1)

    args.stage2_input = str(stage2_input)
    args.output = str(output_dir)
    triplets = load_triplets(args.stage2_input)
    if not triplets:
        print(f"No triplets in {args.stage2_input}")
        sys.exit(1)

    model_path = stage2_task_finetune(model, triplets, args)

    # Export ONNX
    if not args.skip_onnx:
        export_onnx(model_path, args.output)

    print(f"\nDone! To benchmark:")
    print(f"  CODELENS_MODEL_DIR={Path(args.output) / 'onnx'} \\")
    print(f"  python3 benchmarks/embedding-quality.py .")


if __name__ == "__main__":
    main()
