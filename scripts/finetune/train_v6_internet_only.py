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
import math
import os
import sys
import random
import numpy as np
from collections import OrderedDict
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
ROOT = SCRIPT_DIR.parent.parent
INPUT = SCRIPT_DIR / "csn_runtime_format.jsonl"
OUTPUT_DIR = SCRIPT_DIR / "output" / "v6-internet"
MAX_SEQ_LENGTH = 128
DEFAULT_MPS_PADDING_BUCKETS = (64, 96, 128, 160, 192, 224, 256, 320, 384, 448, 512)


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
        os.environ.setdefault("PYTORCH_MPS_FAST_MATH", "1")
        os.environ.setdefault("PYTORCH_MPS_PREFER_METAL", "1")

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
    if resolve_training_device() == "mps":
        return 0
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


def effective_train_batch_size(requested_batch_size: int, pair_count: int, train_device: str) -> int:
    return requested_batch_size


def gradient_accumulation_steps(requested_batch_size: int, effective_batch_size: int) -> int:
    if effective_batch_size <= 0:
        return 1
    return max(1, (requested_batch_size + effective_batch_size - 1) // effective_batch_size)


def resolved_max_seq_length() -> int:
    return max(32, min(512, int(os.environ.get("CODELENS_FINETUNE_MAX_SEQ_LENGTH", MAX_SEQ_LENGTH))))


def resolved_tokenizer_cache_size(train_device: str, loader_workers: int) -> int:
    raw = os.environ.get("CODELENS_TOKENIZER_CACHE_SIZE", "").strip()
    if raw:
        try:
            return max(0, int(raw))
        except ValueError:
            pass
    return 0


def use_static_padding(train_device: str) -> bool:
    return False


def pad_to_multiple_of(train_device: str) -> int | None:
    return 8 if train_device == "mps" else None


def padding_buckets(train_device: str, max_length: int) -> tuple[int, ...]:
    if train_device != "mps":
        return ()
    buckets = [bucket for bucket in DEFAULT_MPS_PADDING_BUCKETS if bucket < max_length]
    buckets.append(max_length)
    return tuple(sorted(set(max(32, bucket) for bucket in buckets)))


def tokenize_rows(tokenizer, texts: list[str], *, max_length: int) -> list[dict]:
    encoded = tokenizer(
        texts,
        padding=False,
        truncation=True,
        max_length=max_length,
    )
    return [
        {key: value[index] for key, value in encoded.items()}
        for index in range(len(texts))
    ]


def pad_token_rows(
    tokenizer,
    token_rows: list[dict],
    *,
    max_length: int,
    train_device: str,
    pad_multiple: int | None,
) -> tuple[dict, int | None]:
    buckets = padding_buckets(train_device, max_length)
    if buckets:
        batch_max = max((len(row.get("input_ids", [])) for row in token_rows), default=0)
        target_length = next((bucket for bucket in buckets if batch_max <= bucket), max_length)
        return (
            tokenizer.pad(
                token_rows,
                padding="max_length",
                max_length=target_length,
                pad_to_multiple_of=pad_multiple,
                return_tensors="pt",
            ),
            target_length,
        )
    return (
        tokenizer.pad(
            token_rows,
            padding=True,
            max_length=max_length,
            pad_to_multiple_of=pad_multiple,
            return_tensors="pt",
        ),
        None,
    )


def optimizer_steps_per_epoch(
    num_examples: int,
    micro_batch_size: int,
    grad_accum_steps: int,
) -> int:
    effective_batch = max(1, micro_batch_size * max(1, grad_accum_steps))
    return max(1, math.ceil(num_examples / effective_batch))


class CachedSentenceDataCollator:
    """Tokenizer-caching collator with bucketed padding for MPS."""

    def __init__(self, tokenizer, *, max_length: int, train_device: str, cache_size: int):
        self.tokenizer = tokenizer
        self.max_length = max_length
        self.static_padding = use_static_padding(train_device)
        self.pad_to_multiple = pad_to_multiple_of(train_device)
        self.padding_buckets = padding_buckets(train_device, max_length)
        self.bucketed_padding = bool(self.padding_buckets)
        self.cache_size = max(0, cache_size)
        self._cache: OrderedDict[str, dict[str, list[int]]] = OrderedDict()
        self.valid_label_columns = ["label", "labels", "score", "scores"]

    def _tokenize_rows(self, texts: list[str]) -> list[dict[str, list[int]]]:
        return tokenize_rows(
            self.tokenizer,
            texts,
            max_length=self.max_length,
        )

    def _pad_rows(self, rows: list[dict[str, list[int]]]):
        padded, _target_length = pad_token_rows(
            self.tokenizer,
            rows,
            max_length=self.max_length,
            train_device="mps" if self.bucketed_padding else "cpu",
            pad_multiple=self.pad_to_multiple,
        )
        return padded

    def _remember(self, text: str, encoded_row: dict[str, list[int]]) -> None:
        if self.cache_size <= 0:
            return
        self._cache[text] = {
            key: list(value)
            for key, value in encoded_row.items()
        }
        self._cache.move_to_end(text)
        while len(self._cache) > self.cache_size:
            self._cache.popitem(last=False)

    def _encode_texts(self, texts: list[str]) -> dict:
        if self.cache_size <= 0:
            return self._pad_rows(self._tokenize_rows(texts))

        misses = []
        seen_misses = set()
        for text in texts:
            if text in self._cache:
                self._cache.move_to_end(text)
                continue
            if text not in seen_misses:
                misses.append(text)
                seen_misses.add(text)

        if misses:
            tokenized_misses = self._tokenize_rows(misses)
            for text, encoded_row in zip(misses, tokenized_misses, strict=False):
                self._remember(text, encoded_row)

        return self._pad_rows([self._cache[text] for text in texts])

    def __call__(self, features: list[dict]) -> dict:
        import torch

        if not features:
            return {}

        batch = {}
        column_names = list(features[0].keys())
        for label_column in self.valid_label_columns:
            if label_column in column_names:
                batch["label"] = torch.tensor([row[label_column] for row in features])
                column_names.remove(label_column)
                break

        sentence_columns = sorted(
            [name for name in column_names if name.startswith("sentence_")],
            key=lambda name: int(name.split("_", 1)[1]),
        )
        for column_name in sentence_columns:
            encoded = self._encode_texts([row[column_name] for row in features])
            for key, value in encoded.items():
                batch[f"{column_name}_{key}"] = value
        return batch


def pretokenized_batches(
    tokenizer,
    texts: list[str],
    *,
    batch_size: int,
    max_length: int,
    train_device: str,
) -> list[dict]:
    batches = []
    for i in range(0, len(texts), batch_size):
        token_rows = tokenize_rows(
            tokenizer,
            texts[i : i + batch_size],
            max_length=max_length,
        )
        padded, _target_length = pad_token_rows(
            tokenizer,
            token_rows,
            max_length=max_length,
            train_device=train_device,
            pad_multiple=pad_to_multiple_of(train_device),
        )
        batches.append(padded)
    return batches


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
            batch,
            padding=True,
            truncation=True,
            max_length=resolved_max_seq_length(),
            return_tensors="np",
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
    tokenized_batches = pretokenized_batches(
        student_tokenizer,
        texts,
        batch_size=batch_size,
        max_length=resolved_max_seq_length(),
        train_device=device.type,
    )

    for epoch in range(epochs):
        total_loss = 0.0
        batches = 0
        for batch_index, i in enumerate(range(0, len(texts), batch_size)):
            batch_targets = torch.from_numpy(teacher_embeddings[i : i + batch_size]).to(
                device=device,
                dtype=torch.float32,
            )
            inputs = {
                key: value.to(device)
                for key, value in tokenized_batches[batch_index].items()
            }

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
    micro_batch_size = effective_train_batch_size(batch_size, pair_count, train_device)
    grad_accum_steps = gradient_accumulation_steps(batch_size, micro_batch_size)
    loader_workers = recommended_loader_workers(pair_count)
    print(
        "  Trainer dataloading: "
        f"batch_sampler={BatchSamplers.NO_DUPLICATES.value} workers={loader_workers} "
        f"micro_batch={micro_batch_size} grad_accum={grad_accum_steps} "
        f"effective_batch={micro_batch_size * grad_accum_steps}"
    )
    collator = CachedSentenceDataCollator(
        student.tokenizer,
        max_length=resolved_max_seq_length(),
        train_device=train_device,
        cache_size=resolved_tokenizer_cache_size(train_device, loader_workers),
    )
    print(
        "  Tokenization path: "
        f"static_padding={collator.static_padding} "
        f"bucketed_padding={collator.bucketed_padding} "
        f"padding_buckets={list(collator.padding_buckets)} "
        f"pad_to_multiple_of={collator.pad_to_multiple} "
        f"cache_size={collator.cache_size}"
    )
    loss = losses.MultipleNegativesRankingLoss(model=student)

    model_output = OUTPUT_DIR / "model"
    model_output.mkdir(parents=True, exist_ok=True)

    steps_per_epoch = optimizer_steps_per_epoch(
        len(train_dataset),
        micro_batch_size,
        grad_accum_steps,
    )
    warmup = int(steps_per_epoch * epochs * 0.1)
    training_args = SentenceTransformerTrainingArguments(
        output_dir=str(OUTPUT_DIR / "checkpoints"),
        per_device_train_batch_size=micro_batch_size,
        per_device_eval_batch_size=micro_batch_size,
        batch_sampler=BatchSamplers.NO_DUPLICATES,
        num_train_epochs=epochs,
        gradient_accumulation_steps=grad_accum_steps,
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
        data_collator=collator,
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
    student.max_seq_length = resolved_max_seq_length()
    print(f"Student max_seq_length: {student.max_seq_length}")

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
