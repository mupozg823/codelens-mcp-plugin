#!/usr/bin/env python3
"""Fine-tune MiniLM-L12 embedding model on CodeLens training pairs.

Input:  scripts/finetune/training_pairs.jsonl (from collect_training_data.py)
Output: scripts/finetune/output/model/ (PyTorch) + output/onnx/ (ONNX INT8)

Usage:
  pip install sentence-transformers onnx onnxruntime optimum
  python scripts/finetune/finetune_embedding.py
  python scripts/finetune/finetune_embedding.py --epochs 5 --batch-size 16
"""

import argparse
import json
import os
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
ROOT = SCRIPT_DIR.parent.parent
TRAINING_DATA = SCRIPT_DIR / "training_pairs.jsonl"
OUTPUT_DIR = SCRIPT_DIR / "output"


def parse_args():
    parser = argparse.ArgumentParser(description="Fine-tune embedding model")
    parser.add_argument("--input", default=str(TRAINING_DATA))
    parser.add_argument("--output", default=str(OUTPUT_DIR))
    parser.add_argument(
        "--base-model", default="sentence-transformers/all-MiniLM-L12-v2"
    )
    parser.add_argument("--epochs", type=int, default=3)
    parser.add_argument("--batch-size", type=int, default=16)
    parser.add_argument("--warmup-ratio", type=float, default=0.1)
    parser.add_argument("--lr", type=float, default=2e-5)
    parser.add_argument("--skip-onnx", action="store_true", help="Skip ONNX export")
    return parser.parse_args()


def load_triplets(path):
    """Load training triplets from JSONL."""
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


def train(args):
    try:
        from sentence_transformers import SentenceTransformer, InputExample, losses
        from sentence_transformers.evaluation import TripletEvaluator
        from torch.utils.data import DataLoader
    except ImportError:
        print("Install dependencies: pip install sentence-transformers torch")
        sys.exit(1)

    triplets = load_triplets(args.input)
    if not triplets:
        print(f"No triplets found in {args.input}")
        sys.exit(1)

    print(f"Loaded {len(triplets)} triplets")
    print(f"Base model: {args.base_model}")

    # Split: 90% train, 10% eval
    split = int(len(triplets) * 0.9)
    train_triplets = triplets[:split]
    eval_triplets = triplets[split:]

    # Build examples
    train_examples = [InputExample(texts=[q, p, n]) for q, p, n in train_triplets]

    # Model
    model = SentenceTransformer(args.base_model)
    train_dataloader = DataLoader(
        train_examples, shuffle=True, batch_size=args.batch_size
    )
    train_loss = losses.TripletLoss(model=model)

    # Evaluator
    evaluator = None
    if eval_triplets:
        evaluator = TripletEvaluator(
            anchors=[t[0] for t in eval_triplets],
            positives=[t[1] for t in eval_triplets],
            negatives=[t[2] for t in eval_triplets],
            name="eval",
        )

    # Train
    model_output = Path(args.output) / "model"
    model_output.mkdir(parents=True, exist_ok=True)

    warmup_steps = int(len(train_dataloader) * args.epochs * args.warmup_ratio)
    print(
        f"Training: {args.epochs} epochs, batch {args.batch_size}, "
        f"lr {args.lr}, warmup {warmup_steps} steps"
    )

    model.fit(
        train_objectives=[(train_dataloader, train_loss)],
        epochs=args.epochs,
        warmup_steps=warmup_steps,
        evaluator=evaluator,
        evaluation_steps=max(1, len(train_dataloader) // 2),
        output_path=str(model_output),
        optimizer_params={"lr": args.lr},
    )

    print(f"\nModel saved to {model_output}")
    return model_output


def export_onnx(model_path, output_dir):
    """Export fine-tuned model to ONNX INT8 for fastembed."""
    try:
        from optimum.onnxruntime import ORTModelForFeatureExtraction
        from optimum.onnxruntime.configuration import AutoQuantizationConfig
        from transformers import AutoTokenizer
    except ImportError:
        print("Install: pip install optimum[onnxruntime] transformers")
        return None

    onnx_dir = Path(output_dir) / "onnx"
    onnx_dir.mkdir(parents=True, exist_ok=True)

    print(f"\nExporting to ONNX INT8...")

    # Load and export
    model = ORTModelForFeatureExtraction.from_pretrained(str(model_path), export=True)
    tokenizer = AutoTokenizer.from_pretrained(str(model_path))

    # Quantize to INT8
    qconfig = AutoQuantizationConfig.avx512_vnni(is_static=False)
    model.save_pretrained(str(onnx_dir))
    tokenizer.save_pretrained(str(onnx_dir))

    # Quantize
    from optimum.onnxruntime import ORTQuantizer

    quantizer = ORTQuantizer.from_pretrained(str(onnx_dir))
    quantizer.quantize(save_dir=str(onnx_dir), quantization_config=qconfig)

    print(f"ONNX INT8 model saved to {onnx_dir}")

    # Rename for CodeLens compatibility
    onnx_model = onnx_dir / "model_quantized.onnx"
    target = onnx_dir / "model.onnx"
    if onnx_model.exists() and not target.exists():
        onnx_model.rename(target)
        print(f"Renamed to {target}")

    return onnx_dir


def main():
    args = parse_args()

    if not Path(args.input).exists():
        print(f"Training data not found: {args.input}")
        print("Run collect_training_data.py first:")
        print(f"  python {SCRIPT_DIR}/collect_training_data.py")
        sys.exit(1)

    # Train
    model_path = train(args)

    # Export ONNX
    if not args.skip_onnx:
        onnx_path = export_onnx(model_path, args.output)
        if onnx_path:
            print(f"\nTo use in CodeLens:")
            print(f"  export CODELENS_MODEL_DIR={onnx_path.parent}")
            print(f"  # Then run embedding-quality.py to compare")


if __name__ == "__main__":
    main()
