#!/usr/bin/env python3
"""V6 Training: Internet-only high-quality data with EXACT runtime format.

Input: csn_runtime_format.jsonl (63K+ pairs from code-search-net/code_search_net)
Loss: MNRL (MultipleNegativesRankingLoss) — NEVER CosineSimilarityLoss
Model: all-MiniLM-L12-v2 → distill from teacher → fine-tune with MNRL

NO local data. Internet high-quality data ONLY.
"""

import ctypes
import hashlib
import json
import os
import sys
import random
import numpy as np
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
ROOT = SCRIPT_DIR.parent.parent
INPUT = SCRIPT_DIR / "csn_runtime_format.jsonl"
OUTPUT_DIR = SCRIPT_DIR / "output" / "v6-internet"


def apple_cpu_topology() -> dict:
    if sys.platform != "darwin":
        return {}

    def read_sysctl(name: str) -> int | None:
        try:
            libc = ctypes.CDLL("libc.dylib", use_errno=True)
            value = ctypes.c_uint(0)
            size = ctypes.c_size_t(ctypes.sizeof(value))
            rc = libc.sysctlbyname(
                name.encode("utf-8"),
                ctypes.byref(value),
                ctypes.byref(size),
                None,
                0,
            )
            if rc != 0 or size.value != ctypes.sizeof(value):
                return None
            return int(value.value)
        except Exception:
            return None

    return {
        "perf_cores": read_sysctl("hw.perflevel0.physicalcpu") or 0,
        "efficiency_cores": read_sysctl("hw.perflevel1.physicalcpu") or 0,
        "logical_cores": read_sysctl("hw.ncpu") or (os.cpu_count() or 1),
    }


def configure_process_runtime() -> dict:
    topology = apple_cpu_topology()
    perf_cores = int(topology.get("perf_cores", 0)) or (os.cpu_count() or 1)
    torch_threads = max(1, min(8, perf_cores))

    os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")
    os.environ.setdefault("OMP_NUM_THREADS", str(torch_threads))
    if sys.platform == "darwin":
        os.environ.setdefault("VECLIB_MAXIMUM_THREADS", str(torch_threads))

    try:
        import torch

        torch.set_num_threads(torch_threads)
        try:
            torch.set_num_interop_threads(1)
        except RuntimeError:
            pass
    except ModuleNotFoundError:
        pass

    return {
        "topology": topology,
        "torch_threads": torch_threads,
    }


def resolve_training_device() -> str:
    import torch

    if torch.backends.mps.is_available():
        return "mps"
    return "cpu"


def recommended_loader_workers(num_examples: int) -> int:
    cpu_count = os.cpu_count() or 1
    if num_examples < 20_000:
        return 0
    if sys.platform == "darwin":
        topology = apple_cpu_topology()
        efficiency_cores = int(topology.get("efficiency_cores", 0))
        if efficiency_cores > 0:
            return max(1, min(4, efficiency_cores))
        return max(1, min(4, cpu_count // 4))
    return max(1, min(8, cpu_count // 2))


def resolve_teacher_providers() -> list[str]:
    # Measured on Apple Silicon for this teacher model:
    # CPU EP has lower load time and lower single-batch latency than partial CoreML execution.
    return ["CPUExecutionProvider"]


def build_teacher_session_config() -> dict:
    if sys.platform == "darwin":
        topology = apple_cpu_topology()
        perf_cores = int(topology.get("perf_cores", 0)) or (os.cpu_count() or 1)
        intra_threads = max(1, min(8, perf_cores))
    else:
        cpu_count = os.cpu_count() or 1
        intra_threads = max(1, min(8, cpu_count // 2 or 1))
    return {
        "intra_op_threads": intra_threads,
        "inter_op_threads": 1,
        "execution_mode": "ORT_SEQUENTIAL",
        "graph_optimization_level": "ORT_ENABLE_ALL",
    }


def recommended_teacher_batch_size(teacher_providers: list[str]) -> int:
    if teacher_providers == ["CPUExecutionProvider"]:
        return 16
    return 32


def iter_retrieval_pairs(path: str):
    with Path(path).open() as f:
        for line in f:
            obj = json.loads(line)
            query = obj.get("query", "").strip()
            positive = obj.get("positive", "").strip()
            if query and positive:
                yield query, positive


def count_retrieval_pairs(path: str) -> int:
    return sum(1 for _ in iter_retrieval_pairs(path))


def collect_positive_texts(path: str, limit: int) -> list[str]:
    texts = []
    for _query, positive in iter_retrieval_pairs(path):
        texts.append(positive)
        if len(texts) >= limit:
            break
    random.shuffle(texts)
    return texts


def load_retrieval_dataset(path: str):
    from datasets import Dataset

    dataset = Dataset.from_json(path, keep_in_memory=False)
    drop_columns = [name for name in dataset.column_names if name not in {"query", "positive"}]
    if drop_columns:
        dataset = dataset.remove_columns(drop_columns)
    dataset = dataset.rename_column("query", "sentence_0")
    dataset = dataset.rename_column("positive", "sentence_1")
    return dataset


def load_teacher(teacher_dir):
    """Load CodeSearchNet ONNX model as teacher."""
    import onnxruntime as ort
    from optimum.onnxruntime import ORTModelForFeatureExtraction
    from transformers import AutoTokenizer

    tokenizer = AutoTokenizer.from_pretrained(teacher_dir)
    providers = resolve_teacher_providers()
    session_config = build_teacher_session_config()
    session_options = ort.SessionOptions()
    session_options.intra_op_num_threads = session_config["intra_op_threads"]
    session_options.inter_op_num_threads = session_config["inter_op_threads"]
    session_options.execution_mode = ort.ExecutionMode.ORT_SEQUENTIAL
    session_options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
    model = ORTModelForFeatureExtraction.from_pretrained(
        teacher_dir,
        subfolder="onnx",
        file_name="model_qint8_arm64.onnx",
        providers=providers,
        session_options=session_options,
    )
    active = []
    session = getattr(model, "model", None)
    if session is not None and hasattr(session, "get_providers"):
        active = session.get_providers()
    return model, tokenizer, active, session_config


def teacher_cache_path(texts: list[str], teacher_dir: str, teacher_providers: list[str]) -> Path:
    key = hashlib.sha256()
    key.update(str(Path(teacher_dir).resolve()).encode("utf-8"))
    key.update("|".join(teacher_providers).encode("utf-8"))
    model_file = Path(teacher_dir) / "onnx" / "model_qint8_arm64.onnx"
    if model_file.exists():
        stat = model_file.stat()
        key.update(str(stat.st_size).encode("utf-8"))
        key.update(str(stat.st_mtime_ns).encode("utf-8"))
    for text in texts:
        key.update(text.encode("utf-8"))
        key.update(b"\n")
    cache_dir = OUTPUT_DIR / "cache"
    cache_dir.mkdir(parents=True, exist_ok=True)
    return cache_dir / f"teacher-embeddings-{key.hexdigest()[:16]}.npy"


def teacher_embed(model, tokenizer, texts, batch_size: int):
    if not texts:
        return np.empty((0, 0), dtype=np.float32)

    def pool_teacher_batch(token_embeddings, attention_mask):
        mask = attention_mask.astype(token_embeddings.dtype, copy=False)
        pooled = np.einsum("bsd,bs->bd", token_embeddings, mask, optimize=True)
        counts = mask.sum(axis=1, dtype=token_embeddings.dtype, keepdims=True)
        np.maximum(counts, 1e-9, out=counts)
        np.divide(pooled, counts, out=pooled)
        norms = np.linalg.norm(pooled, axis=1, keepdims=True)
        np.maximum(norms, 1e-9, out=norms)
        np.divide(pooled, norms, out=pooled)
        return pooled

    all_embeddings = None
    offset = 0
    for i in range(0, len(texts), batch_size):
        batch = texts[i : i + batch_size]
        inputs = tokenizer(
            batch, padding=True, truncation=True, max_length=512, return_tensors="np"
        )
        outputs = model(**{k: v for k, v in inputs.items()})
        pooled = pool_teacher_batch(outputs.last_hidden_state, inputs["attention_mask"])
        if all_embeddings is None:
            all_embeddings = np.empty(
                (len(texts), pooled.shape[1]),
                dtype=pooled.dtype,
            )
        batch_len = pooled.shape[0]
        all_embeddings[offset : offset + batch_len] = pooled
        offset += batch_len
    return all_embeddings


def load_or_compute_teacher_embeddings(model, tokenizer, texts, teacher_dir, teacher_providers):
    cache_path = teacher_cache_path(texts, teacher_dir, teacher_providers)
    if cache_path.exists():
        print(f"  Teacher cache hit: {cache_path}")
        return np.load(cache_path, allow_pickle=False, mmap_mode="r")

    if model is None or tokenizer is None:
        print("  Teacher cache miss: loading teacher ONNX...")
        model, tokenizer, _active, _session = load_teacher(teacher_dir)

    teacher_batch_size = recommended_teacher_batch_size(teacher_providers)
    print(f"  Teacher batch size: {teacher_batch_size}")
    embeddings = teacher_embed(model, tokenizer, texts, batch_size=teacher_batch_size)
    np.save(cache_path, embeddings, allow_pickle=False)
    print(f"  Teacher cache saved: {cache_path}")
    return embeddings


def stage1_distill(
    student,
    pairs_path,
    teacher_dir,
    teacher_providers,
    batch_size=32,
    epochs=3,
):
    """Stage 1: Align student with teacher via MSE on positive texts."""
    import torch

    # Use positive texts for distillation alignment
    texts = collect_positive_texts(pairs_path, limit=3000)

    print(f"\n=== Stage 1: Distillation ({len(texts)} texts, {epochs} epochs) ===")
    print("  Generating teacher embeddings...")
    teacher_embeddings = load_or_compute_teacher_embeddings(
        None,
        None,
        texts,
        teacher_dir,
        teacher_providers,
    )
    print(f"  Teacher shape: {teacher_embeddings.shape}")

    device = torch.device(resolve_training_device())
    print(f"  Device: {device}")
    student = student.to(device)
    student_model = student[0].auto_model
    student_tokenizer = student.tokenizer

    optimizer = torch.optim.AdamW(student_model.parameters(), lr=2e-5)
    mse_loss = torch.nn.MSELoss()

    for epoch in range(epochs):
        total_loss = 0.0
        batches = 0
        for i in range(0, len(texts), batch_size):
            batch_texts = texts[i : i + batch_size]
            batch_targets = torch.from_numpy(teacher_embeddings[i : i + batch_size]).to(
                device=device,
                dtype=torch.float32,
            )

            inputs = student_tokenizer(
                batch_texts,
                padding=True,
                truncation=True,
                max_length=512,
                return_tensors="pt",
            ).to(device)

            outputs = student_model(**inputs)
            token_embs = outputs.last_hidden_state
            mask = inputs["attention_mask"].unsqueeze(-1).float()
            student_embs = (token_embs * mask).sum(1) / mask.sum(1).clamp(min=1e-9)
            student_embs = torch.nn.functional.normalize(student_embs, p=2, dim=1)

            loss = mse_loss(student_embs, batch_targets)
            loss.backward()
            optimizer.step()
            optimizer.zero_grad()

            total_loss += loss.item()
            batches += 1

        avg = total_loss / max(batches, 1)
        print(f"  Epoch {epoch + 1}/{epochs}: MSE = {avg:.6f}")

    del teacher_embeddings
    if device.type == "mps":
        torch.mps.empty_cache()
    return student


def stage2_mnrl(student, pairs_path, pair_count, batch_size=32, epochs=5):
    """Stage 2: MNRL fine-tuning. NEVER use CosineSimilarityLoss."""
    from sentence_transformers import losses
    from sentence_transformers.trainer import SentenceTransformerTrainer
    from sentence_transformers.training_args import BatchSamplers, SentenceTransformerTrainingArguments

    print(f"\n=== Stage 2: MNRL Fine-tuning ({pair_count} pairs, {epochs} epochs) ===")
    train_device = resolve_training_device()
    student = student.to(train_device)
    print(f"  Device: {train_device}")

    train_dataset = load_retrieval_dataset(pairs_path)
    loader_workers = recommended_loader_workers(pair_count)
    print(
        "  Trainer dataloading: "
        f"batch_sampler={BatchSamplers.NO_DUPLICATES.value} workers={loader_workers} batch_size={batch_size}"
    )
    loss = losses.MultipleNegativesRankingLoss(model=student)

    model_output = OUTPUT_DIR / "model"
    model_output.mkdir(parents=True, exist_ok=True)

    steps_per_epoch = max(1, len(train_dataset) // batch_size)
    warmup = int(steps_per_epoch * epochs * 0.1)
    training_args = SentenceTransformerTrainingArguments(
        output_dir=str(OUTPUT_DIR / "checkpoints"),
        per_device_train_batch_size=batch_size,
        per_device_eval_batch_size=batch_size,
        batch_sampler=BatchSamplers.NO_DUPLICATES,
        num_train_epochs=epochs,
        warmup_steps=warmup,
        learning_rate=1e-5,
        save_strategy="no",
        dataloader_num_workers=loader_workers,
        dataloader_persistent_workers=loader_workers > 0,
        dataloader_prefetch_factor=2 if loader_workers > 0 else None,
        dataloader_pin_memory=(train_device == "cpu"),
        use_mps_device=(train_device == "mps"),
        use_cpu=(train_device == "cpu"),
        disable_tqdm=False,
        report_to=[],
        logging_steps=max(1, min(500, steps_per_epoch)),
    )
    trainer = SentenceTransformerTrainer(
        model=student,
        args=training_args,
        train_dataset=train_dataset,
        loss=loss,
    )
    trainer.train()
    trainer.save_model(str(model_output))
    print(f"Stage 2 complete → {model_output}")
    return model_output


def export_onnx(model_path):
    from optimum.onnxruntime import ORTModelForFeatureExtraction
    from transformers import AutoTokenizer

    onnx_dir = OUTPUT_DIR / "onnx"
    onnx_dir.mkdir(parents=True, exist_ok=True)

    model = ORTModelForFeatureExtraction.from_pretrained(str(model_path), export=True)
    tokenizer = AutoTokenizer.from_pretrained(str(model_path))
    model.save_pretrained(str(onnx_dir))
    tokenizer.save_pretrained(str(onnx_dir))
    print(f"ONNX exported: {onnx_dir}")
    return onnx_dir


def main():
    from sentence_transformers import SentenceTransformer

    runtime = configure_process_runtime()
    print(f"Runtime placement: {runtime}")

    # Load data
    pair_count = count_retrieval_pairs(str(INPUT))
    print(f"Loaded {pair_count} pairs (internet-only)")

    teacher_dir = str(ROOT / "models" / "codelens-code-search" / "arm64")
    teacher_providers = resolve_teacher_providers()
    teacher_session = build_teacher_session_config()
    print(f"Teacher providers: {teacher_providers}")
    print(f"Teacher session: {teacher_session}")

    # Load student
    print("Loading student (all-MiniLM-L12-v2)...")
    student = SentenceTransformer("sentence-transformers/all-MiniLM-L12-v2")

    # Stage 1: Distillation
    student = stage1_distill(
        student,
        str(INPUT),
        teacher_dir,
        teacher_providers,
    )
    # Stage 2: MNRL
    model_path = stage2_mnrl(student, str(INPUT), pair_count)

    # Export ONNX
    export_onnx(model_path)

    print(f"\n=== V6 Training Complete ===")
    print(f"Copy to test:")
    print(f"  mkdir -p /tmp/codelens-v6/codesearch")
    print(f"  cp {OUTPUT_DIR}/onnx/* /tmp/codelens-v6/codesearch/")


if __name__ == "__main__":
    main()
