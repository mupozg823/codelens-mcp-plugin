#!/usr/bin/env python3
"""Knowledge distillation: CodeSearchNet (ONNX teacher) → MiniLM-L12 (PyTorch student).

The bundled CodeSearchNet ONNX model cannot be fine-tuned directly (no PyTorch weights).
Instead, we use it as a teacher to distill into all-MiniLM-L12-v2, then fine-tune
the student on project-specific data.

Stage 1: Distill CodeSearchNet → student (align embedding spaces)
Stage 2: Fine-tune student on (query, code) pairs with MNRL loss

Output: scripts/finetune/output/distill/model/ + onnx/
"""

import argparse
import json
import os
import shutil
import tempfile
import numpy as np
import random
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
ROOT = SCRIPT_DIR.parent.parent
DEFAULT_GENERAL_INPUT = SCRIPT_DIR / "training_pairs_augmented.jsonl"
DEFAULT_CODEX_INPUT = SCRIPT_DIR / "training_pairs_codex.jsonl"


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--profile", choices=["general", "codex"], default="general")
    parser.add_argument(
        "--student-model", default="sentence-transformers/all-MiniLM-L12-v2"
    )
    parser.add_argument(
        "--teacher-dir", default=str(ROOT / "models" / "codelens-code-search" / "arm64")
    )
    parser.add_argument(
        "--distill-texts",
        type=int,
        default=3000,
        help="Number of code texts for distillation alignment",
    )
    parser.add_argument("--distill-epochs", type=int, default=3)
    parser.add_argument("--finetune-input", default="")
    parser.add_argument("--finetune-epochs", type=int, default=5)
    parser.add_argument("--batch-size", type=int, default=32)
    parser.add_argument("--output", default="")
    parser.add_argument("--skip-onnx", action="store_true")
    return parser.parse_args()


def resolve_finetune_input(profile: str, explicit_input: str) -> Path:
    if explicit_input:
        return Path(explicit_input)
    if profile == "codex":
        return DEFAULT_CODEX_INPUT
    return DEFAULT_GENERAL_INPUT


def resolve_output(profile: str, explicit_output: str) -> Path:
    if explicit_output:
        return Path(explicit_output)
    if profile == "codex":
        return SCRIPT_DIR / "output" / "codex-distill"
    return SCRIPT_DIR / "output" / "distill"


def load_teacher(teacher_dir):
    """Load CodeSearchNet ONNX model as teacher."""
    from optimum.onnxruntime import ORTModelForFeatureExtraction
    from transformers import AutoTokenizer

    onnx_path = os.path.join(teacher_dir, "onnx", "model_qint8_arm64.onnx")
    tmp = tempfile.mkdtemp()
    for f in [
        "config.json",
        "tokenizer.json",
        "special_tokens_map.json",
        "tokenizer_config.json",
        "vocab.txt",
    ]:
        src = os.path.join(teacher_dir, f)
        if os.path.exists(src):
            shutil.copy(src, tmp)
    shutil.copy(onnx_path, os.path.join(tmp, "model.onnx"))

    tokenizer = AutoTokenizer.from_pretrained(tmp)
    model = ORTModelForFeatureExtraction.from_pretrained(tmp, file_name="model.onnx")
    return model, tokenizer, tmp


def teacher_embed(model, tokenizer, texts, batch_size=64):
    """Generate embeddings from teacher model."""
    all_embeddings = []
    for i in range(0, len(texts), batch_size):
        batch = texts[i : i + batch_size]
        inputs = tokenizer(
            batch, padding=True, truncation=True, max_length=512, return_tensors="np"
        )
        outputs = model(**{k: v for k, v in inputs.items()})
        # Mean pooling
        token_embeddings = outputs.last_hidden_state
        attention_mask = inputs["attention_mask"]
        mask_expanded = np.expand_dims(attention_mask, -1)
        summed = np.sum(token_embeddings * mask_expanded, axis=1)
        counts = np.clip(np.sum(mask_expanded, axis=1), 1e-9, None)
        embeddings = summed / counts
        # L2 normalize
        norms = np.linalg.norm(embeddings, axis=1, keepdims=True)
        embeddings = embeddings / np.clip(norms, 1e-9, None)
        all_embeddings.append(embeddings)
    return np.vstack(all_embeddings)


def collect_code_texts(n=3000):
    """Collect code snippets from the codebase for distillation."""
    texts = []
    code_dir = ROOT / "crates"
    for rs_file in code_dir.rglob("*.rs"):
        try:
            content = rs_file.read_text()
        except Exception:
            continue
        for line in content.split("\n"):
            stripped = line.strip()
            if len(stripped) > 30 and not stripped.startswith("//"):
                texts.append(stripped)
            if len(texts) >= n * 3:
                break
        if len(texts) >= n * 3:
            break

    # Also add symbol signatures from the quality dataset
    dataset_path = ROOT / "benchmarks" / "embedding-quality-dataset.json"
    if dataset_path.exists():
        with open(dataset_path) as f:
            for entry in json.load(f):
                texts.append(entry["query"])
                texts.append(f"function {entry['expected_symbol']}")

    random.shuffle(texts)
    return texts[:n]


def stage1_distill(student, teacher_model, teacher_tokenizer, texts, args):
    """Align student embeddings with teacher via MSE loss."""
    import torch
    from torch.utils.data import DataLoader, TensorDataset

    print(
        f"\n=== Stage 1: Distillation ({len(texts)} texts, {args.distill_epochs} epochs) ==="
    )

    # Get teacher embeddings
    print("  Generating teacher embeddings...")
    teacher_embeddings = teacher_embed(teacher_model, teacher_tokenizer, texts)
    print(f"  Teacher embeddings shape: {teacher_embeddings.shape}")

    # Get student embeddings and compute alignment loss
    device = torch.device("mps" if torch.backends.mps.is_available() else "cpu")
    student_model = student[0].auto_model.to(device)
    student_tokenizer = student.tokenizer

    target_tensor = torch.tensor(teacher_embeddings, dtype=torch.float32).to(device)

    optimizer = torch.optim.AdamW(student_model.parameters(), lr=2e-5)
    mse_loss = torch.nn.MSELoss()

    for epoch in range(args.distill_epochs):
        total_loss = 0.0
        batches = 0
        for i in range(0, len(texts), args.batch_size):
            batch_texts = texts[i : i + args.batch_size]
            batch_targets = target_tensor[i : i + args.batch_size]

            inputs = student_tokenizer(
                batch_texts,
                padding=True,
                truncation=True,
                max_length=512,
                return_tensors="pt",
            ).to(device)

            outputs = student_model(**inputs)
            # Mean pooling
            token_embs = outputs.last_hidden_state
            mask = inputs["attention_mask"].unsqueeze(-1).float()
            student_embs = (token_embs * mask).sum(1) / mask.sum(1).clamp(min=1e-9)
            # L2 normalize
            student_embs = torch.nn.functional.normalize(student_embs, p=2, dim=1)

            loss = mse_loss(student_embs, batch_targets)
            loss.backward()
            optimizer.step()
            optimizer.zero_grad()

            total_loss += loss.item()
            batches += 1

        avg_loss = total_loss / max(batches, 1)
        print(f"  Epoch {epoch + 1}/{args.distill_epochs}: MSE loss = {avg_loss:.6f}")

    student[0].auto_model = student_model.cpu()
    print("Stage 1 complete.")
    return student


def stage2_finetune(student, triplets_path, args):
    """Fine-tune with SPENCER-style distillation losses (NOT contrastive/MNRL).

    SPENCER (arxiv:2508.00546) finding: contrastive loss during distillation
    HURTS performance by -8.6%. Use only:
    - CosineSimilarityLoss (dual-modality: preserve query-code similarity)
    - MSELoss via teacher alignment is already done in Stage 1.

    Previous version used MNRL here — that was the anti-pattern SPENCER warned about.
    """
    from sentence_transformers import InputExample, losses
    from torch.utils.data import DataLoader

    print(
        f"\n=== Stage 2: CosineSimilarity Fine-tuning ({args.finetune_epochs} epochs) ==="
    )
    print("  (SPENCER: contrastive/MNRL removed — uses cosine similarity only)")

    pairs = []
    with open(triplets_path) as f:
        for line in f:
            obj = json.loads(line.strip())
            if obj.get("positive"):
                pairs.append((obj["query"], obj["positive"]))

    print(f"  Loaded {len(pairs)} query-positive pairs")

    # SPENCER dual-modality loss: CosineSimilarityLoss with label=1.0 (positive pairs)
    examples = [InputExample(texts=[q, p], label=1.0) for q, p in pairs]
    dataloader = DataLoader(examples, shuffle=True, batch_size=args.batch_size)
    loss = losses.CosineSimilarityLoss(model=student)

    model_output = Path(args.output) / "model"
    model_output.mkdir(parents=True, exist_ok=True)

    warmup = int(len(dataloader) * args.finetune_epochs * 0.1)
    student.fit(
        train_objectives=[(dataloader, loss)],
        epochs=args.finetune_epochs,
        warmup_steps=warmup,
        output_path=str(model_output),
        optimizer_params={"lr": 1e-5},
        show_progress_bar=True,
    )
    print(f"Stage 2 complete. Model saved to {model_output}")
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
    finetune_input = resolve_finetune_input(args.profile, args.finetune_input)
    output_dir = resolve_output(args.profile, args.output)

    from sentence_transformers import SentenceTransformer

    if not finetune_input.exists():
        if args.profile == "codex":
            raise SystemExit(
                f"Codex dataset not found: {finetune_input}\nBuild it first: python {SCRIPT_DIR}/build_codex_dataset.py"
            )
        raise SystemExit(f"Fine-tune input not found: {finetune_input}")

    args.finetune_input = str(finetune_input)
    args.output = str(output_dir)

    # Load teacher (CodeSearchNet ONNX)
    print("Loading teacher (CodeSearchNet ONNX)...")
    teacher_model, teacher_tokenizer, teacher_tmp = load_teacher(args.teacher_dir)

    # Load student
    print(f"Loading student ({args.student_model})...")
    student = SentenceTransformer(args.student_model)

    # Stage 1: Distillation
    texts = collect_code_texts(args.distill_texts)
    print(f"Collected {len(texts)} code texts for distillation")
    student = stage1_distill(student, teacher_model, teacher_tokenizer, texts, args)

    # Cleanup teacher
    shutil.rmtree(teacher_tmp, ignore_errors=True)

    # Stage 2: MNRL fine-tuning
    model_path = stage2_finetune(student, args.finetune_input, args)

    # Export ONNX
    if not args.skip_onnx:
        export_onnx(model_path, args.output)

    print(f"\nBenchmark:")
    print(f"  mkdir -p /tmp/codelens-distill/codesearch")
    print(
        f"  cp {args.output}/onnx/{{model.onnx,tokenizer.json,config.json,special_tokens_map.json,tokenizer_config.json}} /tmp/codelens-distill/codesearch/"
    )
    print(
        f"  CODELENS_MODEL_DIR=/tmp/codelens-distill python3 benchmarks/embedding-quality.py ."
    )


if __name__ == "__main__":
    main()
