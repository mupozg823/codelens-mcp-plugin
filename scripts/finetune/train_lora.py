#!/usr/bin/env python3
"""LoRA language-specific adapter training.

Uses V6 internet-only data (real file paths, runtime format).
Trains LoRA adapters on top of the 3-layer compressed base model.

Usage:
    python train_lora.py --lang python
    python train_lora.py --lang javascript
"""

import argparse
import json
from pathlib import Path

import torch
from peft import LoraConfig, get_peft_model, TaskType
from sentence_transformers import SentenceTransformer, InputExample, losses
from torch.utils.data import DataLoader

SCRIPT_DIR = Path(__file__).parent
BASE_MODEL = SCRIPT_DIR / "output" / "compressed-3layer" / "model"


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--lang",
        required=True,
        choices=["python", "javascript", "go", "java", "ruby", "php"],
    )
    parser.add_argument("--rank", type=int, default=16)
    parser.add_argument("--epochs", type=int, default=3)
    parser.add_argument("--batch-size", type=int, default=32)
    parser.add_argument("--lr", type=float, default=2e-4)
    return parser.parse_args()


def main():
    args = parse_args()
    data_path = SCRIPT_DIR / f"lora_v6_{args.lang}.jsonl"
    output_dir = SCRIPT_DIR / "output" / f"lora-{args.lang}"

    if not data_path.exists():
        raise SystemExit(f"Data not found: {data_path}")

    # Load data
    pairs = []
    with data_path.open() as f:
        for line in f:
            obj = json.loads(line)
            pairs.append((obj["query"], obj["positive"]))
    print(f"Loaded {len(pairs)} pairs for {args.lang}")

    # Load base model
    print(f"Loading base model: {BASE_MODEL}")
    model = SentenceTransformer(str(BASE_MODEL))

    # Apply LoRA
    lora_config = LoraConfig(
        task_type=TaskType.FEATURE_EXTRACTION,
        r=args.rank,
        lora_alpha=args.rank * 2,
        lora_dropout=0.1,
        target_modules=["query", "value"],
    )

    auto_model = model[0].auto_model
    peft_model = get_peft_model(auto_model, lora_config)
    trainable = sum(p.numel() for p in peft_model.parameters() if p.requires_grad)
    total = sum(p.numel() for p in peft_model.parameters())
    print(f"LoRA params: {trainable:,} / {total:,} ({trainable/total*100:.2f}%)")
    model[0].auto_model = peft_model

    # Train with MNRL (NEVER CosineSimilarityLoss)
    examples = [InputExample(texts=[q, p]) for q, p in pairs]
    dataloader = DataLoader(examples, shuffle=True, batch_size=args.batch_size)
    loss_fn = losses.MultipleNegativesRankingLoss(model=model)

    model_output = output_dir / "model"
    model_output.mkdir(parents=True, exist_ok=True)

    warmup = int(len(dataloader) * args.epochs * 0.1)
    model.fit(
        train_objectives=[(dataloader, loss_fn)],
        epochs=args.epochs,
        warmup_steps=warmup,
        output_path=str(model_output),
        optimizer_params={"lr": args.lr},
        show_progress_bar=True,
    )
    print(f"LoRA adapter saved: {model_output}")

    # Export merged ONNX
    print("Merging LoRA weights and exporting ONNX...")
    merged = peft_model.merge_and_unload()
    model[0].auto_model = merged
    model.save(str(model_output))

    from optimum.onnxruntime import ORTModelForFeatureExtraction
    from transformers import AutoTokenizer
    from onnxruntime.quantization import quantize_dynamic, QuantType

    onnx_dir = output_dir / "onnx"
    onnx_dir.mkdir(parents=True, exist_ok=True)
    ort_model = ORTModelForFeatureExtraction.from_pretrained(
        str(model_output), export=True
    )
    tokenizer = AutoTokenizer.from_pretrained(str(model_output))
    ort_model.save_pretrained(str(onnx_dir))
    tokenizer.save_pretrained(str(onnx_dir))

    # Quantize
    fp32 = onnx_dir / "model.onnx"
    fp32_backup = onnx_dir / "model_fp32.onnx"
    int8 = onnx_dir / "model_qint8.onnx"
    quantize_dynamic(str(fp32), str(int8), weight_type=QuantType.QInt8)
    if fp32_backup.exists():
        fp32_backup.unlink()
    fp32.replace(fp32_backup)
    int8.replace(fp32)
    manifest = {
        "model_name": f"MiniLM-L12-CodeSearchNet-LoRA-{args.lang}",
        "base_model": str(BASE_MODEL),
        "fine_tuned_from": str(data_path),
        "adapter_type": "lora",
        "lora_merged_from": str(model_output),
        "export_backend": "onnx",
    }
    (onnx_dir / "model-manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n",
        encoding="utf-8",
    )

    import os

    print(f"INT8: {os.path.getsize(fp32)/1024/1024:.1f}MB")
    print(f"\nDone: {args.lang} LoRA adapter")


if __name__ == "__main__":
    main()
