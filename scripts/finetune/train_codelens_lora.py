#!/usr/bin/env python3
"""Train a CodeLens retrieval LoRA adapter and export an INT8 ONNX runtime model.

The script keeps heavyweight ML imports inside the training path so `--dry-run`
can validate data, configuration, and promotion-gate metadata in lightweight CI.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import random
from collections import Counter
from pathlib import Path
from typing import Any, NamedTuple


SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent.parent
DEFAULT_PIPELINE = SCRIPT_DIR / "pipelines" / "v12-sanitized"
DEFAULT_TRAIN_DATA = DEFAULT_PIPELINE / "train.jsonl"
DEFAULT_VALIDATION_DATA = DEFAULT_PIPELINE / "validation.jsonl"
DEFAULT_OUTPUT_DIR = SCRIPT_DIR / "output" / "codelens-lora"
DEFAULT_BASE_MODEL = "sentence-transformers/all-MiniLM-L12-v2"
DEFAULT_MODEL_NAME = "MiniLM-L12-CodeLens-LoRA-INT8"
DEFAULT_TEACHER_DIR = REPO_ROOT / "crates" / "codelens-engine" / "models" / "codesearch"
DEFAULT_TEACHER_LABEL = "MiniLM-L12-CodeSearchNet-INT8"


class TrainingPair(NamedTuple):
    query: str
    positive: str
    metadata: dict[str, Any]


def parse_target_modules(value: str | list[str]) -> list[str]:
    if isinstance(value, list):
        modules = value
    else:
        modules = value.split(",")
    normalized = [item.strip() for item in modules if item.strip()]
    if not normalized:
        raise SystemExit("--target-modules must include at least one module name")
    return normalized


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Train and quantize a CodeLens-optimized semantic LoRA model."
    )
    parser.add_argument("--train-data", default=str(DEFAULT_TRAIN_DATA))
    parser.add_argument("--validation-data", default=str(DEFAULT_VALIDATION_DATA))
    parser.add_argument("--base-model", default=DEFAULT_BASE_MODEL)
    parser.add_argument("--teacher-dir", default=str(DEFAULT_TEACHER_DIR))
    parser.add_argument("--teacher-label", default=DEFAULT_TEACHER_LABEL)
    parser.add_argument("--output-dir", default=str(DEFAULT_OUTPUT_DIR))
    parser.add_argument("--model-name", default=DEFAULT_MODEL_NAME)
    parser.add_argument("--query-field", default="query")
    parser.add_argument("--positive-field", default="positive")
    parser.add_argument("--rank", type=int, default=16)
    parser.add_argument("--alpha", type=int, default=32)
    parser.add_argument("--dropout", type=float, default=0.05)
    parser.add_argument("--target-modules", default="query,value")
    parser.add_argument("--epochs", type=int, default=2)
    parser.add_argument("--batch-size", type=int, default=32)
    parser.add_argument("--lr", type=float, default=2e-4)
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument(
        "--max-train-rows",
        type=int,
        default=0,
        help="Cap training rows for smoke runs. 0 means all valid rows.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Validate inputs and write training-plan.json without importing ML deps.",
    )
    parser.add_argument(
        "--no-quantize",
        action="store_true",
        help="Keep the exported ONNX model in fp32 instead of dynamic INT8.",
    )
    args = parser.parse_args(argv)
    args.target_modules = parse_target_modules(args.target_modules)
    return args


def load_pairs(
    path: str | Path,
    *,
    query_field: str = "query",
    positive_field: str = "positive",
    max_rows: int = 0,
) -> list[TrainingPair]:
    data_path = Path(path)
    if not data_path.exists():
        raise SystemExit(f"Training data not found: {data_path}")

    pairs: list[TrainingPair] = []
    with data_path.open(encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, start=1):
            if not line.strip():
                continue
            try:
                obj = json.loads(line)
            except json.JSONDecodeError as exc:
                raise SystemExit(f"Invalid JSONL row {data_path}:{line_number}: {exc}")
            query = str(obj.get(query_field, "")).strip()
            positive = str(obj.get(positive_field, "")).strip()
            if not query or not positive:
                continue
            metadata = {
                key: value
                for key, value in obj.items()
                if key not in {query_field, positive_field}
            }
            pairs.append(TrainingPair(query=query, positive=positive, metadata=metadata))
            if max_rows > 0 and len(pairs) >= max_rows:
                break
    return pairs


def training_stats(pairs: list[TrainingPair]) -> dict[str, Any]:
    if not pairs:
        return {
            "rows": 0,
            "avg_query_chars": 0.0,
            "avg_positive_chars": 0.0,
            "max_query_chars": 0,
            "max_positive_chars": 0,
            "languages": {},
            "query_types": {},
            "sources": {},
        }
    languages = Counter(str(pair.metadata.get("language", "")) for pair in pairs)
    query_types = Counter(str(pair.metadata.get("query_type", "")) for pair in pairs)
    sources = Counter(str(pair.metadata.get("source", "")) for pair in pairs)
    languages.pop("", None)
    query_types.pop("", None)
    sources.pop("", None)
    return {
        "rows": len(pairs),
        "avg_query_chars": round(
            sum(len(pair.query) for pair in pairs) / len(pairs), 2
        ),
        "avg_positive_chars": round(
            sum(len(pair.positive) for pair in pairs) / len(pairs), 2
        ),
        "max_query_chars": max(len(pair.query) for pair in pairs),
        "max_positive_chars": max(len(pair.positive) for pair in pairs),
        "languages": dict(sorted(languages.items())),
        "query_types": dict(sorted(query_types.items())),
        "sources": dict(sorted(sources.items())),
    }


def _path_string(value: str | Path) -> str:
    return str(Path(value))


def file_sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def teacher_identity(teacher_dir: str | Path, teacher_label: str) -> dict[str, Any]:
    model_dir = Path(teacher_dir).expanduser().resolve()
    model_path = model_dir / "model.onnx"
    config_path = model_dir / "config.json"
    identity: dict[str, Any] = {
        "teacher_model": teacher_label,
        "teacher_model_dir": str(model_dir),
        "teacher_model_path": str(model_path),
        "teacher_available": model_path.exists(),
    }
    if model_path.exists():
        identity["teacher_sha256"] = file_sha256(model_path)
        identity["teacher_size_bytes"] = model_path.stat().st_size
    if config_path.exists():
        config = json.loads(config_path.read_text(encoding="utf-8"))
        identity["teacher_num_hidden_layers"] = config.get("num_hidden_layers")
        identity["teacher_hidden_size"] = config.get("hidden_size")
    return identity


def promotion_gate_command(args: argparse.Namespace) -> list[str]:
    onnx_dir = Path(args.output_dir) / "onnx"
    manifest_path = onnx_dir / "model-manifest.json"
    return [
        "python3",
        "scripts/finetune/promotion_gate.py",
        "--candidate-onnx-dir",
        _path_string(onnx_dir),
        "--candidate-label",
        args.model_name,
        "--candidate-manifest",
        str(manifest_path),
    ]


def build_runtime_manifest(
    args: argparse.Namespace,
    *,
    quantized: bool,
    train_stats: dict[str, Any],
    validation_stats: dict[str, Any],
) -> dict[str, Any]:
    output_dir = Path(args.output_dir)
    return {
        "schema_version": "codelens-lora-model-v1",
        "model_name": args.model_name,
        "base_model": str(args.base_model),
        "fine_tuned_from": str(args.train_data),
        **teacher_identity(args.teacher_dir, args.teacher_label),
        "adapter_type": "lora",
        "lora_rank": args.rank,
        "lora_alpha": args.alpha,
        "lora_dropout": args.dropout,
        "lora_target_modules": parse_target_modules(args.target_modules),
        "lora_merged_from": _path_string(output_dir / "model"),
        "export_backend": "onnx",
        "quantization": "dynamic-int8" if quantized else "none",
        "loss": "MultipleNegativesRankingLoss",
        "train_stats": train_stats,
        "validation_stats": validation_stats,
        "promotion_gate_command": promotion_gate_command(args),
    }


def write_training_plan(
    args: argparse.Namespace,
    *,
    dry_run: bool,
    train_stats: dict[str, Any],
    validation_stats: dict[str, Any],
) -> Path:
    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    manifest = build_runtime_manifest(
        args,
        quantized=not args.no_quantize,
        train_stats=train_stats,
        validation_stats=validation_stats,
    )
    plan = {
        "dry_run": dry_run,
        "train_data": str(args.train_data),
        "validation_data": str(args.validation_data),
        "epochs": args.epochs,
        "batch_size": args.batch_size,
        "learning_rate": args.lr,
        "seed": args.seed,
        **manifest,
    }
    plan_path = output_dir / "training-plan.json"
    plan_path.write_text(
        json.dumps(plan, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return plan_path


def load_training_dependencies():
    try:
        import torch
        from peft import LoraConfig, TaskType, get_peft_model
        from sentence_transformers import InputExample, SentenceTransformer, losses
        from torch.utils.data import DataLoader
    except ImportError as exc:
        raise SystemExit(
            "Missing ML training dependency. Install sentence-transformers, peft, "
            "torch, optimum[onnxruntime], transformers, and onnxruntime before "
            "running without --dry-run."
        ) from exc
    return {
        "torch": torch,
        "LoraConfig": LoraConfig,
        "TaskType": TaskType,
        "get_peft_model": get_peft_model,
        "InputExample": InputExample,
        "SentenceTransformer": SentenceTransformer,
        "losses": losses,
        "DataLoader": DataLoader,
    }


def export_onnx(
    model_output: Path,
    onnx_dir: Path,
    *,
    quantize: bool,
) -> None:
    try:
        from onnxruntime.quantization import QuantType, quantize_dynamic
        from optimum.onnxruntime import ORTModelForFeatureExtraction
        from transformers import AutoTokenizer
    except ImportError as exc:
        raise SystemExit(
            "Missing ONNX export dependency. Install optimum[onnxruntime], "
            "transformers, and onnxruntime."
        ) from exc

    onnx_dir.mkdir(parents=True, exist_ok=True)
    ort_model = ORTModelForFeatureExtraction.from_pretrained(
        str(model_output), export=True
    )
    tokenizer = AutoTokenizer.from_pretrained(str(model_output))
    ort_model.save_pretrained(str(onnx_dir))
    tokenizer.save_pretrained(str(onnx_dir))

    if not quantize:
        return

    fp32 = onnx_dir / "model.onnx"
    fp32_backup = onnx_dir / "model_fp32.onnx"
    int8 = onnx_dir / "model_qint8.onnx"
    quantize_dynamic(str(fp32), str(int8), weight_type=QuantType.QInt8)
    if fp32_backup.exists():
        fp32_backup.unlink()
    fp32.replace(fp32_backup)
    int8.replace(fp32)


def train(args: argparse.Namespace, pairs: list[TrainingPair]) -> None:
    deps = load_training_dependencies()
    torch = deps["torch"]
    random.seed(args.seed)
    torch.manual_seed(args.seed)

    model = deps["SentenceTransformer"](str(args.base_model))
    if not hasattr(model[0], "auto_model"):
        raise SystemExit("Base SentenceTransformer does not expose model[0].auto_model")

    lora_config = deps["LoraConfig"](
        task_type=deps["TaskType"].FEATURE_EXTRACTION,
        r=args.rank,
        lora_alpha=args.alpha,
        lora_dropout=args.dropout,
        target_modules=args.target_modules,
    )
    peft_model = deps["get_peft_model"](model[0].auto_model, lora_config)
    trainable = sum(
        parameter.numel() for parameter in peft_model.parameters() if parameter.requires_grad
    )
    total = sum(parameter.numel() for parameter in peft_model.parameters())
    print(
        f"LoRA trainable params: {trainable:,} / {total:,} "
        f"({trainable / total * 100:.2f}%)"
    )
    model[0].auto_model = peft_model

    examples = [
        deps["InputExample"](texts=[pair.query, pair.positive]) for pair in pairs
    ]
    dataloader = deps["DataLoader"](
        examples,
        shuffle=True,
        batch_size=args.batch_size,
    )
    loss_fn = deps["losses"].MultipleNegativesRankingLoss(model=model)

    model_output = Path(args.output_dir) / "model"
    model_output.mkdir(parents=True, exist_ok=True)
    warmup_steps = int(len(dataloader) * args.epochs * 0.1)
    model.fit(
        train_objectives=[(dataloader, loss_fn)],
        epochs=args.epochs,
        warmup_steps=warmup_steps,
        output_path=str(model_output),
        optimizer_params={"lr": args.lr},
        show_progress_bar=True,
    )

    model[0].auto_model = peft_model.merge_and_unload()
    model.save(str(model_output))
    export_onnx(
        model_output,
        Path(args.output_dir) / "onnx",
        quantize=not args.no_quantize,
    )


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    pairs = load_pairs(
        args.train_data,
        query_field=args.query_field,
        positive_field=args.positive_field,
        max_rows=args.max_train_rows,
    )
    if not pairs:
        raise SystemExit("No valid training pairs found")
    validation_pairs = load_pairs(
        args.validation_data,
        query_field=args.query_field,
        positive_field=args.positive_field,
    )
    train_stats = training_stats(pairs)
    validation_stats = training_stats(validation_pairs)
    plan_path = write_training_plan(
        args,
        dry_run=args.dry_run,
        train_stats=train_stats,
        validation_stats=validation_stats,
    )
    print(f"Wrote training plan: {plan_path}")
    print(f"Training rows: {train_stats['rows']}")
    print(f"Validation rows: {validation_stats['rows']}")
    if args.dry_run:
        return 0

    train(args, pairs)
    manifest = build_runtime_manifest(
        args,
        quantized=not args.no_quantize,
        train_stats=train_stats,
        validation_stats=validation_stats,
    )
    manifest_path = Path(args.output_dir) / "onnx" / "model-manifest.json"
    manifest_path.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(f"Wrote runtime manifest: {manifest_path}")
    print("Run promotion gate:")
    print(" ".join(promotion_gate_command(args)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
