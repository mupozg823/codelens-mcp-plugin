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
import ctypes
import hashlib
import json
import math
import os
import sys
import random
import re
from collections import OrderedDict
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
ROOT = SCRIPT_DIR.parent.parent
DEFAULT_GENERAL_INPUT = SCRIPT_DIR / "training_pairs_augmented.jsonl"
DEFAULT_CODEX_INPUT = SCRIPT_DIR / "training_pairs_codex.jsonl"
DEFAULT_MPS_PADDING_BUCKETS = (64, 96, 128, 160, 192, 224, 256, 320, 384, 448, 512)


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
        "--teacher-provider",
        choices=["auto", "cpu", "coreml"],
        default="auto",
        help="Execution provider for ONNX teacher inference (auto defaults to CPU for this model)",
    )
    parser.add_argument(
        "--distill-texts",
        type=int,
        default=3000,
        help="Number of code texts for distillation alignment",
    )
    parser.add_argument("--distill-epochs", type=int, default=3)
    parser.add_argument(
        "--distill-batch-size",
        type=int,
        default=0,
        help="Stage 1 batch size (0 = auto from resource profile)",
    )
    parser.add_argument("--finetune-input", default="")
    parser.add_argument(
        "--pipeline-manifest",
        default="",
        help="Manifest from build_runtime_training_pipeline.py",
    )
    parser.add_argument(
        "--validation-input",
        default="",
        help="Optional held-out retrieval pairs for evaluator",
    )
    parser.add_argument(
        "--distill-input",
        default="",
        help="Optional JSONL file with {'text': ...} rows for Stage 1 distillation",
    )
    parser.add_argument("--finetune-epochs", type=int, default=5)
    parser.add_argument("--batch-size", type=int, default=32)
    parser.add_argument(
        "--eval-batch-size",
        type=int,
        default=0,
        help="Validation/evaluator batch size (0 = auto, separate from train batch).",
    )
    parser.add_argument(
        "--max-seq-length",
        type=int,
        default=128,
        help="Max token length for runtime-aligned texts (data p95: query=82, positive=95, negative=98)",
    )
    parser.add_argument(
        "--stage",
        choices=["all", "distill", "finetune"],
        default="all",
        help="Run the full pipeline, only Stage 1 distillation, or only Stage 2 fine-tuning.",
    )
    parser.add_argument(
        "--max-train-rows",
        type=int,
        default=0,
        help="Limit Stage 2 train rows for safe subset runs (0 = full dataset).",
    )
    parser.add_argument(
        "--max-validation-rows",
        type=int,
        default=0,
        help="Limit validation rows for safe subset runs (0 = full dataset).",
    )
    parser.add_argument(
        "--resource-profile",
        choices=["auto", "balanced"],
        default="auto",
        help="Runtime placement profile (auto = balanced placement defaults)",
    )
    parser.add_argument(
        "--train-device",
        choices=["auto", "cpu", "mps"],
        default="auto",
        help="Torch device for student training",
    )
    parser.add_argument(
        "--loader-workers",
        type=int,
        default=-1,
        help="DataLoader worker count when using plain DataLoader (-1 = auto)",
    )
    parser.add_argument(
        "--torch-threads",
        type=int,
        default=-1,
        help="Torch/OpenMP thread count (-1 = auto from resource profile)",
    )
    parser.add_argument(
        "--seed",
        type=int,
        default=42,
        help="Random seed for distill text selection and trainer shuffles.",
    )
    parser.add_argument("--output", default="")
    parser.add_argument(
        "--evaluation-steps",
        type=int,
        default=0,
        help="Run evaluator every N optimization steps (0 = disable evaluator)",
    )
    parser.add_argument(
        "--use-cached-mnrl",
        action="store_true",
        help="Use CachedMultipleNegativesRankingLoss for larger effective negative sets",
    )
    parser.add_argument(
        "--cached-mnrl-mini-batch-size",
        type=int,
        default=32,
        help="Mini-batch size for CachedMultipleNegativesRankingLoss",
    )
    parser.add_argument(
        "--disable-no-duplicates-loader",
        action="store_true",
        help="Fall back to a plain DataLoader instead of NoDuplicatesDataLoader",
    )
    parser.add_argument(
        "--save-steps",
        type=int,
        default=0,
        help="Checkpoint every N optimization steps (0 = auto).",
    )
    parser.add_argument(
        "--resume-from-checkpoint",
        default="auto",
        help="Checkpoint path, 'auto' for latest checkpoint in output/checkpoints, or 'none'.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Validate inputs and print resolved pipeline without starting training",
    )
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


def load_pipeline_manifest(path: str) -> dict:
    manifest_path = Path(path)
    if not manifest_path.exists():
        raise SystemExit(f"Pipeline manifest not found: {manifest_path}")
    if manifest_path.is_dir():
        manifest_path = manifest_path / "manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    manifest["_manifest_path"] = str(manifest_path)
    return manifest


def resolve_pipeline_inputs(args):
    manifest = None
    if args.pipeline_manifest:
        manifest = load_pipeline_manifest(args.pipeline_manifest)
        args.finetune_input = manifest["train_path"]
        if not args.validation_input:
            args.validation_input = manifest.get("validation_path", "")
        if not args.distill_input:
            args.distill_input = manifest.get("distill_texts_path", "")
    return manifest


def resolved_max_seq_length(requested: int) -> int:
    return max(32, min(512, requested))


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


def resolve_resource_profile(profile_name: str) -> tuple[str, dict]:
    topology = apple_cpu_topology()
    if profile_name == "auto":
        return "balanced", topology
    return profile_name, topology


def default_torch_threads(profile_name: str) -> int:
    cpu_count = os.cpu_count() or 1
    if sys.platform == "darwin":
        topology = apple_cpu_topology()
        perf_cores = int(topology.get("perf_cores", 0)) or cpu_count
        return max(1, min(8, perf_cores))
    return max(1, min(8, cpu_count // 2 or 1))


def configure_process_runtime(profile_name: str, requested_threads: int) -> int:
    threads = (
        requested_threads
        if requested_threads > 0
        else default_torch_threads(profile_name)
    )
    os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")
    os.environ.setdefault("OMP_NUM_THREADS", str(threads))
    if sys.platform == "darwin":
        os.environ.setdefault("VECLIB_MAXIMUM_THREADS", str(threads))
        os.environ.setdefault("PYTORCH_MPS_FAST_MATH", "1")
        os.environ.setdefault("PYTORCH_MPS_PREFER_METAL", "1")

    try:
        import torch

        torch.set_num_threads(threads)
        try:
            torch.set_num_interop_threads(1)
        except RuntimeError:
            pass
    except ModuleNotFoundError:
        pass

    return threads


def configure_random_seed(seed: int) -> None:
    random.seed(seed)
    try:
        import numpy as np

        np.random.seed(seed)
    except ModuleNotFoundError:
        pass

    try:
        import torch

        torch.manual_seed(seed)
        if torch.cuda.is_available():
            torch.cuda.manual_seed_all(seed)
    except ModuleNotFoundError:
        pass


def resolved_tokenizer_cache_size(train_device: str, loader_workers: int) -> int:
    raw = os.environ.get("CODELENS_TOKENIZER_CACHE_SIZE", "").strip()
    if raw:
        try:
            return max(0, int(raw))
        except ValueError:
            pass
    return 0


def use_static_padding(train_device: str) -> bool:
    # MPS benefits from stable tensor shapes, but padding to max_length wastes
    # compute when data is short (avg ~50 tokens vs max_length 256).
    # Use dynamic padding with pad_to_multiple_of instead — gives MPS shape
    # stability without the full max_length waste.
    return False


def pad_to_multiple_of(train_device: str) -> int | None:
    # Pad to multiple of 8 on MPS for stable kernel shapes without full max_length waste.
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


def effective_train_batch_size(
    requested_batch_size: int,
    profile_name: str,
    train_device: str,
    num_examples: int,
) -> int:
    # Do NOT force micro-batch reduction on MPS.  MNRL uses in-batch negatives:
    # batch 64 → 127 candidate docs (with hard negatives), micro 16 → only 31.
    # Gradient accumulation does NOT pool negatives across micro-batches.
    return requested_batch_size


def gradient_accumulation_steps(
    requested_batch_size: int,
    effective_batch_size: int,
) -> int:
    if effective_batch_size <= 0:
        return 1
    return max(
        1, (requested_batch_size + effective_batch_size - 1) // effective_batch_size
    )


def effective_eval_batch_size(
    requested_eval_batch_size: int,
    train_batch_size: int,
    train_device: str,
) -> int:
    if requested_eval_batch_size > 0:
        return requested_eval_batch_size
    if train_device == "mps":
        return max(1, min(train_batch_size, 32))
    return max(1, train_batch_size)


def optimizer_steps_per_epoch(
    num_examples: int,
    micro_batch_size: int,
    grad_accum_steps: int,
) -> int:
    effective_batch = max(1, micro_batch_size * max(1, grad_accum_steps))
    return max(1, math.ceil(num_examples / effective_batch))


def resolved_save_steps(requested_save_steps: int, steps_per_epoch: int) -> int:
    if requested_save_steps > 0:
        return requested_save_steps
    return max(250, min(1000, steps_per_epoch))


def latest_checkpoint_dir(checkpoints_dir: Path) -> Path | None:
    latest_step = -1
    latest_path = None
    for checkpoint_dir in checkpoints_dir.glob("checkpoint-*"):
        if not checkpoint_dir.is_dir():
            continue
        try:
            step = int(checkpoint_dir.name.rsplit("-", 1)[1])
        except (IndexError, ValueError):
            continue
        if step > latest_step:
            latest_step = step
            latest_path = checkpoint_dir
    return latest_path


def resolve_resume_checkpoint(value: str, checkpoints_dir: Path) -> Path | None:
    normalized = value.strip()
    if not normalized or normalized.lower() == "none":
        return None
    if normalized.lower() == "auto":
        return latest_checkpoint_dir(checkpoints_dir)

    resume_path = Path(normalized)
    if not resume_path.exists():
        raise SystemExit(f"Checkpoint not found: {resume_path}")
    return resume_path


def effective_distill_batch_size(
    requested_batch_size: int,
    profile_name: str,
    train_device: str,
) -> int:
    batch_size = requested_batch_size if requested_batch_size > 0 else 16
    if train_device == "mps":
        return min(batch_size, 16)
    return batch_size


def resolve_training_device(device_name: str, *, strict: bool = True) -> str:
    try:
        import torch
    except ModuleNotFoundError:
        if strict:
            raise SystemExit(
                "torch is required to resolve the requested training device"
            )
        if device_name in {"cpu", "mps"}:
            return device_name
        return "auto"

    if device_name == "cpu":
        return "cpu"
    if device_name == "mps":
        if not torch.backends.mps.is_available():
            raise SystemExit("Requested --train-device mps, but MPS is not available")
        return "mps"
    if torch.backends.mps.is_available():
        return "mps"
    return "cpu"


def recommended_loader_workers(requested_workers: int, num_examples: int) -> int:
    if requested_workers >= 0:
        return requested_workers

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


def effective_loader_workers(
    requested_workers: int,
    num_examples: int,
    profile_name: str,
    train_device: str,
) -> int:
    if train_device == "mps":
        return 0
    return recommended_loader_workers(requested_workers, num_examples)


def resolve_teacher_providers(provider_name: str, *, strict: bool = True) -> list[str]:
    try:
        import onnxruntime as ort
    except ModuleNotFoundError:
        if strict:
            raise SystemExit(
                "onnxruntime is required to resolve the requested teacher provider"
            )
        if provider_name == "coreml":
            return ["CoreMLExecutionProvider", "CPUExecutionProvider"]
        if provider_name == "cpu":
            return ["CPUExecutionProvider"]
        return ["auto"]

    available = set(ort.get_available_providers())
    if provider_name == "cpu":
        return ["CPUExecutionProvider"]
    if provider_name == "coreml":
        if "CoreMLExecutionProvider" not in available:
            raise SystemExit(
                "Requested --teacher-provider coreml, but CoreMLExecutionProvider is unavailable"
            )
        return ["CoreMLExecutionProvider", "CPUExecutionProvider"]
    # Measured on Apple Silicon for the bundled teacher model:
    # CPU EP loads faster and runs small-batch inference faster than partial CoreML partitioning.
    # Keep CoreML available as an explicit opt-in, but default auto to CPU for stability.
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


class CachedSentenceDataCollator:
    """Tokenizer-caching collator with bucketed padding for MPS."""

    def __init__(
        self,
        tokenizer,
        *,
        max_length: int,
        train_device: str,
        cache_size: int,
    ):
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

        misses: list[str] = []
        seen_misses: set[str] = set()
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


def load_teacher(teacher_dir, provider_name: str):
    """Load CodeSearchNet ONNX model as teacher."""
    import onnxruntime as ort
    from optimum.onnxruntime import ORTModelForFeatureExtraction
    from transformers import AutoTokenizer

    tokenizer = AutoTokenizer.from_pretrained(teacher_dir)
    providers = resolve_teacher_providers(provider_name)
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


def teacher_cache_path(
    output_dir: str,
    texts: list[str],
    teacher_dir: str,
    teacher_providers: list[str],
    *,
    max_seq_length: int,
) -> Path:
    key = hashlib.sha256()
    key.update(str(Path(teacher_dir).resolve()).encode("utf-8"))
    key.update("|".join(teacher_providers).encode("utf-8"))
    key.update(str(resolved_max_seq_length(max_seq_length)).encode("utf-8"))
    model_file = Path(teacher_dir) / "onnx" / "model_qint8_arm64.onnx"
    if model_file.exists():
        stat = model_file.stat()
        key.update(str(stat.st_size).encode("utf-8"))
        key.update(str(stat.st_mtime_ns).encode("utf-8"))
    for text in texts:
        key.update(text.encode("utf-8"))
        key.update(b"\n")

    cache_dir = Path(output_dir) / "cache"
    cache_dir.mkdir(parents=True, exist_ok=True)
    return cache_dir / f"teacher-embeddings-{key.hexdigest()[:16]}.npy"


def teacher_embed(model, tokenizer, texts, batch_size: int, max_seq_length: int = 128):
    """Generate embeddings from teacher model."""
    import numpy as np

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
            max_length=resolved_max_seq_length(max_seq_length),
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


def load_or_compute_teacher_embeddings(
    model,
    tokenizer,
    texts: list[str],
    args,
    teacher_providers: list[str],
):
    import numpy as np

    cache_path = teacher_cache_path(
        args.output,
        texts,
        args.teacher_dir,
        teacher_providers,
        max_seq_length=args.max_seq_length,
    )
    if cache_path.exists():
        embeddings = np.load(cache_path, allow_pickle=False, mmap_mode="r")
        print(f"  Teacher cache hit: {cache_path}")
        return embeddings

    if model is None or tokenizer is None:
        print("  Teacher cache miss: loading teacher ONNX...")
        model, tokenizer, _active, _session = load_teacher(
            args.teacher_dir,
            args.teacher_provider,
        )

    teacher_batch_size = recommended_teacher_batch_size(teacher_providers)
    print(f"  Teacher batch size: {teacher_batch_size}")
    embeddings = teacher_embed(
        model,
        tokenizer,
        texts,
        batch_size=teacher_batch_size,
        max_seq_length=args.max_seq_length,
    )
    np.save(cache_path, embeddings, allow_pickle=False)
    print(f"  Teacher cache saved: {cache_path}")
    return embeddings


def collect_code_texts(
    n=3000,
    distill_input: str = "",
    finetune_input: str = "",
    *,
    seed: int = 42,
):
    """Collect generic texts for teacher alignment without benchmark leakage."""
    rng = random.Random(seed)
    if distill_input:
        rows = []
        with open(distill_input, encoding="utf-8") as f:
            for line in f:
                obj = json.loads(line)
                text = obj.get("text", "").strip()
                if text:
                    rows.append(text)
        rng.shuffle(rows)
        return rows[:n]

    runtime_texts = []
    if finetune_input and Path(finetune_input).exists():
        with open(finetune_input, encoding="utf-8") as f:
            for line in f:
                obj = json.loads(line)
                query = obj.get("query", "").strip()
                positive = obj.get("positive", "").strip()
                if positive:
                    runtime_texts.append(positive)
                if query:
                    runtime_texts.append(query)
    if runtime_texts:
        rng.shuffle(runtime_texts)
        return runtime_texts[:n]

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

    rng.shuffle(texts)
    return texts[:n]


def stage1_distill(student, texts, args, teacher_providers):
    """Align student embeddings with teacher via MSE loss."""
    import torch

    print(
        f"\n=== Stage 1: Distillation ({len(texts)} texts, {args.distill_epochs} epochs) ==="
    )

    # Get teacher embeddings
    print("  Generating teacher embeddings...")
    teacher_embeddings = load_or_compute_teacher_embeddings(
        None,
        None,
        texts,
        args,
        teacher_providers,
    )
    print(f"  Teacher embeddings shape: {teacher_embeddings.shape}")

    # Get student embeddings and compute alignment loss
    device = torch.device(resolve_training_device(args.train_device))
    student = student.to(device)
    student_model = student[0].auto_model
    student_tokenizer = student.tokenizer
    print(f"  Student device: {device}")

    optimizer = torch.optim.AdamW(student_model.parameters(), lr=2e-5)
    mse_loss = torch.nn.MSELoss()
    distill_batch_size = effective_distill_batch_size(
        args.distill_batch_size or args.batch_size,
        args._resource_profile,
        str(device),
    )
    tokenized_batches = pretokenized_batches(
        student_tokenizer,
        texts,
        batch_size=distill_batch_size,
        max_length=resolved_max_seq_length(args.max_seq_length),
        train_device=device.type,
    )

    for epoch in range(args.distill_epochs):
        total_loss = 0.0
        batches = 0
        for batch_index, i in enumerate(range(0, len(texts), distill_batch_size)):
            batch_targets = torch.from_numpy(
                teacher_embeddings[i : i + distill_batch_size]
            ).to(
                device=device,
                dtype=torch.float32,
            )
            inputs = {
                key: value.to(device)
                for key, value in tokenized_batches[batch_index].items()
            }

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

    del teacher_embeddings
    if device.type == "mps":
        torch.mps.empty_cache()
    print("Stage 1 complete.")
    return student


def iter_retrieval_rows(path: str, max_rows: int = 0):
    with open(path, encoding="utf-8") as f:
        yielded = 0
        for line in f:
            obj = json.loads(line.strip())
            query = obj.get("query", "").strip()
            positive = obj.get("positive", "").strip()
            if query and positive:
                yield obj
                yielded += 1
                if max_rows > 0 and yielded >= max_rows:
                    return


def iter_retrieval_pairs(path: str, max_rows: int = 0):
    for obj in iter_retrieval_rows(path, max_rows=max_rows):
        yield obj["query"].strip(), obj["positive"].strip()


def count_retrieval_pairs(path: str, max_rows: int = 0) -> int:
    return sum(1 for _ in iter_retrieval_pairs(path, max_rows=max_rows))


def count_rows_with_hard_negatives(path: str, max_rows: int = 0) -> int:
    count = 0
    for obj in iter_retrieval_rows(path, max_rows=max_rows):
        if any(
            obj.get(name, "").strip()
            for name in obj.keys()
            if name == "negative" or re.fullmatch(r"negative_\d+", name)
        ):
            count += 1
    return count


def subset_jsonl_path(path: str, max_rows: int, output_dir: str, tag: str) -> str:
    source = Path(path)
    if max_rows <= 0:
        return str(source)

    cache_dir = Path(output_dir) / "cache" / "subsets"
    cache_dir.mkdir(parents=True, exist_ok=True)
    subset = cache_dir / f"{source.stem}-{tag}-{max_rows}.jsonl"
    if subset.exists():
        return str(subset)

    with source.open(encoding="utf-8") as src, subset.open(
        "w", encoding="utf-8"
    ) as dst:
        kept = 0
        for line in src:
            obj = json.loads(line.strip())
            query = obj.get("query", "").strip()
            positive = obj.get("positive", "").strip()
            if not query or not positive:
                continue
            dst.write(json.dumps(obj, ensure_ascii=False) + "\n")
            kept += 1
            if kept >= max_rows:
                break
    return str(subset)


def load_retrieval_dataset(
    path: str, *, max_rows: int = 0, output_dir: str = "", tag: str = "train"
):
    from datasets import Dataset

    dataset_path = subset_jsonl_path(
        path, max_rows, output_dir or str(ROOT / "scripts" / "finetune" / "output"), tag
    )
    dataset = Dataset.from_json(dataset_path, keep_in_memory=False)
    keep_columns = ["query", "positive"]
    negative_columns = sorted(
        [
            name
            for name in dataset.column_names
            if name == "negative" or re.fullmatch(r"negative_\d+", name)
        ],
        key=lambda name: (0 if name == "negative" else int(name.split("_")[1])),
    )
    keep_columns.extend(negative_columns)
    drop_columns = [
        name for name in dataset.column_names if name not in set(keep_columns)
    ]
    if drop_columns:
        dataset = dataset.remove_columns(drop_columns)
    dataset = dataset.rename_column("query", "sentence_0")
    dataset = dataset.rename_column("positive", "sentence_1")
    for idx, column in enumerate(negative_columns, start=2):
        dataset = dataset.rename_column(column, f"sentence_{idx}")
    return dataset


def build_validation_evaluator(
    validation_input: str, batch_size: int, *, max_rows: int = 0
):
    if not validation_input:
        return None
    path = Path(validation_input)
    if not path.exists():
        raise SystemExit(f"Validation input not found: {path}")

    from sentence_transformers.evaluation import InformationRetrievalEvaluator

    queries = {}
    corpus = {}
    relevant_docs = {}
    positive_to_doc_id = {}

    for idx, (query, positive) in enumerate(
        iter_retrieval_pairs(str(path), max_rows=max_rows)
    ):
        qid = f"q{idx}"
        if positive not in positive_to_doc_id:
            positive_to_doc_id[positive] = f"d{len(positive_to_doc_id)}"
            corpus[positive_to_doc_id[positive]] = positive
        doc_id = positive_to_doc_id[positive]
        queries[qid] = query
        relevant_docs[qid] = {doc_id}

    if not queries:
        return None

    return InformationRetrievalEvaluator(
        queries=queries,
        corpus=corpus,
        relevant_docs=relevant_docs,
        mrr_at_k=[10],
        accuracy_at_k=[1, 3, 5, 10],
        batch_size=batch_size,
        name="validation_ir",
        write_csv=True,
        write_predictions=False,
    )


def stage2_finetune(student, triplets_path, args):
    """Fine-tune with MultipleNegativesRankingLoss (MNRL).

    SPENCER correction: The paper's "no contrastive in distillation" refers to the
    teacher→student alignment stage (Stage 1 MSE), NOT the fine-tuning stage.
    Stage 2 fine-tuning NEEDS contrastive loss (MNRL) for discriminative power.

    Verified: CosineSimilarityLoss alone → loss 0.0, MRR 0.094 (model loses all
    discriminative ability). MNRL → loss 0.057, MRR 0.620 (correct).
    """
    from sentence_transformers import losses
    from sentence_transformers.trainer import SentenceTransformerTrainer
    from sentence_transformers.training_args import (
        BatchSamplers,
        SentenceTransformerTrainingArguments,
    )

    print(f"\n=== Stage 2: MNRL Fine-tuning ({args.finetune_epochs} epochs) ===")

    train_dataset = load_retrieval_dataset(
        triplets_path,
        max_rows=args.max_train_rows,
        output_dir=args.output,
        tag="train",
    )
    pair_count = len(train_dataset)

    print(f"  Loaded {pair_count} query-positive pairs")
    train_device = resolve_training_device(args.train_device)
    student = student.to(train_device)
    print(f"  Student device: {train_device}")
    train_batch_size = effective_train_batch_size(
        args.batch_size,
        args._resource_profile,
        train_device,
        pair_count,
    )
    grad_accum_steps = gradient_accumulation_steps(args.batch_size, train_batch_size)
    loader_workers = effective_loader_workers(
        args.loader_workers,
        pair_count,
        args._resource_profile,
        train_device,
    )
    batch_sampler = (
        BatchSamplers.BATCH_SAMPLER
        if args.disable_no_duplicates_loader
        else BatchSamplers.NO_DUPLICATES
    )
    print(
        "  Trainer dataloading: "
        f"batch_sampler={batch_sampler.value} workers={loader_workers} "
        f"micro_batch={train_batch_size} grad_accum={grad_accum_steps} "
        f"effective_batch={train_batch_size * grad_accum_steps}"
    )
    eval_batch_size = effective_eval_batch_size(
        args.eval_batch_size,
        train_batch_size,
        train_device,
    )
    print(f"  Eval batching: batch_size={eval_batch_size}")
    collator = CachedSentenceDataCollator(
        student.tokenizer,
        max_length=resolved_max_seq_length(args.max_seq_length),
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

    use_cached_mnrl = args.use_cached_mnrl and train_device != "mps"
    if args.use_cached_mnrl and train_device == "mps":
        print(
            "  Loss: CachedMNRL requested but disabled on MPS due to torch.mps incompatibility"
        )

    if use_cached_mnrl:
        loss = losses.CachedMultipleNegativesRankingLoss(
            model=student,
            mini_batch_size=args.cached_mnrl_mini_batch_size,
        )
        print(
            "  Loss: CachedMultipleNegativesRankingLoss "
            f"(mini_batch_size={args.cached_mnrl_mini_batch_size})"
        )
    else:
        loss = losses.MultipleNegativesRankingLoss(model=student)
        print("  Loss: MultipleNegativesRankingLoss")

    model_output = Path(args.output) / "model"
    model_output.mkdir(parents=True, exist_ok=True)

    steps_per_epoch = optimizer_steps_per_epoch(
        len(train_dataset),
        train_batch_size,
        grad_accum_steps,
    )
    warmup = int(steps_per_epoch * args.finetune_epochs * 0.1)
    evaluator = None
    if args.validation_input and args.evaluation_steps > 0:
        evaluator = build_validation_evaluator(
            args.validation_input,
            eval_batch_size,
            max_rows=args.max_validation_rows,
        )

    checkpoints_dir = Path(args.output) / "checkpoints"
    checkpoints_dir.mkdir(parents=True, exist_ok=True)
    save_steps = resolved_save_steps(args.save_steps, steps_per_epoch)
    resume_checkpoint = resolve_resume_checkpoint(
        args.resume_from_checkpoint,
        checkpoints_dir,
    )
    print(
        "  Checkpoint policy: "
        f"save_steps={save_steps} "
        f"resume_from={resume_checkpoint if resume_checkpoint else 'none'}"
    )

    training_args = SentenceTransformerTrainingArguments(
        output_dir=str(checkpoints_dir),
        per_device_train_batch_size=train_batch_size,
        per_device_eval_batch_size=eval_batch_size,
        batch_sampler=batch_sampler,
        num_train_epochs=args.finetune_epochs,
        gradient_accumulation_steps=grad_accum_steps,
        warmup_steps=warmup,
        learning_rate=1e-5,
        save_strategy="steps",
        save_steps=save_steps,
        save_total_limit=2,
        eval_strategy="steps" if evaluator and args.evaluation_steps > 0 else "no",
        eval_steps=(
            args.evaluation_steps if evaluator and args.evaluation_steps > 0 else None
        ),
        dataloader_num_workers=loader_workers,
        dataloader_persistent_workers=loader_workers > 0,
        dataloader_prefetch_factor=2 if loader_workers > 0 else None,
        dataloader_pin_memory=(train_device == "cpu"),
        use_mps_device=(train_device == "mps"),
        use_cpu=(train_device == "cpu"),
        disable_tqdm=False,
        report_to=[],
        logging_steps=max(1, min(500, steps_per_epoch)),
        seed=args.seed,
        data_seed=args.seed,
    )
    trainer = SentenceTransformerTrainer(
        model=student,
        args=training_args,
        train_dataset=train_dataset,
        loss=loss,
        evaluator=evaluator,
        data_collator=collator,
    )
    trainer.train(
        resume_from_checkpoint=str(resume_checkpoint) if resume_checkpoint else None
    )
    trainer.save_model(str(model_output))
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
    manifest = resolve_pipeline_inputs(args)
    finetune_input = resolve_finetune_input(args.profile, args.finetune_input)
    output_dir = resolve_output(args.profile, args.output)
    args._resource_profile, topology = resolve_resource_profile(args.resource_profile)
    args._topology = topology
    args._torch_threads = configure_process_runtime(
        args._resource_profile, args.torch_threads
    )
    configure_random_seed(args.seed)

    if not finetune_input.exists():
        if args.profile == "codex":
            raise SystemExit(
                f"Codex dataset not found: {finetune_input}\nBuild it first: python {SCRIPT_DIR}/build_codex_dataset.py"
            )
        raise SystemExit(f"Fine-tune input not found: {finetune_input}")

    args.finetune_input = str(finetune_input)
    args.output = str(output_dir)

    if args.validation_input:
        args.validation_input = str(Path(args.validation_input))
    if args.distill_input:
        args.distill_input = str(Path(args.distill_input))

    if args.dry_run:
        resolved_train_device = resolve_training_device(args.train_device, strict=False)
        dry_run_train_pairs = count_retrieval_pairs(
            args.finetune_input,
            max_rows=args.max_train_rows,
        )
        effective_train_micro_batch = effective_train_batch_size(
            args.batch_size,
            args._resource_profile,
            resolved_train_device,
            dry_run_train_pairs,
        )
        dry_run_resume_checkpoint = resolve_resume_checkpoint(
            args.resume_from_checkpoint,
            Path(args.output) / "checkpoints",
        )
        summary = {
            "finetune_input": args.finetune_input,
            "validation_input": args.validation_input,
            "distill_input": args.distill_input,
            "output": args.output,
            "stage": args.stage,
            "max_train_rows": args.max_train_rows,
            "max_validation_rows": args.max_validation_rows,
            "pipeline_manifest": manifest.get("_manifest_path") if manifest else None,
            "resource_profile_requested": args.resource_profile,
            "resource_profile": args._resource_profile,
            "seed": args.seed,
            "topology": topology,
            "torch_threads": args._torch_threads,
            "train_device_requested": args.train_device,
            "train_device": resolved_train_device,
            "teacher_providers": resolve_teacher_providers(
                args.teacher_provider,
                strict=False,
            ),
            "teacher_session": build_teacher_session_config(),
            "teacher_batch_size": recommended_teacher_batch_size(
                resolve_teacher_providers(
                    args.teacher_provider,
                    strict=False,
                )
            ),
            "max_seq_length": resolved_max_seq_length(args.max_seq_length),
            "tokenizer_static_padding": use_static_padding(resolved_train_device),
            "tokenizer_bucketed_padding": bool(
                padding_buckets(
                    resolved_train_device,
                    resolved_max_seq_length(args.max_seq_length),
                )
            ),
            "tokenizer_padding_buckets": list(
                padding_buckets(
                    resolved_train_device,
                    resolved_max_seq_length(args.max_seq_length),
                )
            ),
            "tokenizer_pad_to_multiple_of": pad_to_multiple_of(resolved_train_device),
            "tokenizer_cache_size": resolved_tokenizer_cache_size(
                resolved_train_device,
                effective_loader_workers(
                    args.loader_workers,
                    dry_run_train_pairs,
                    args._resource_profile,
                    resolved_train_device,
                ),
            ),
            "mps_fast_math": os.environ.get("PYTORCH_MPS_FAST_MATH"),
            "mps_prefer_metal": os.environ.get("PYTORCH_MPS_PREFER_METAL"),
            "effective_train_batch_size": effective_train_batch_size(
                args.batch_size,
                args._resource_profile,
                resolved_train_device,
                dry_run_train_pairs,
            ),
            "effective_eval_batch_size": effective_eval_batch_size(
                args.eval_batch_size,
                effective_train_micro_batch,
                resolved_train_device,
            ),
            "gradient_accumulation_steps": gradient_accumulation_steps(
                args.batch_size,
                effective_train_micro_batch,
            ),
            "optimizer_steps_per_epoch": optimizer_steps_per_epoch(
                dry_run_train_pairs,
                effective_train_micro_batch,
                gradient_accumulation_steps(
                    args.batch_size,
                    effective_train_micro_batch,
                ),
            ),
            "checkpoint_save_steps": resolved_save_steps(
                args.save_steps,
                optimizer_steps_per_epoch(
                    dry_run_train_pairs,
                    effective_train_micro_batch,
                    gradient_accumulation_steps(
                        args.batch_size,
                        effective_train_micro_batch,
                    ),
                ),
            ),
            "checkpoint_resume_from": (
                str(dry_run_resume_checkpoint) if dry_run_resume_checkpoint else None
            ),
            "effective_distill_batch_size": effective_distill_batch_size(
                args.distill_batch_size or args.batch_size,
                args._resource_profile,
                resolved_train_device,
            ),
            "train_pairs": dry_run_train_pairs,
            "train_rows_with_hard_negatives": count_rows_with_hard_negatives(
                args.finetune_input,
                max_rows=args.max_train_rows,
            ),
            "validation_pairs": (
                count_retrieval_pairs(
                    args.validation_input,
                    max_rows=args.max_validation_rows,
                )
                if args.validation_input
                else 0
            ),
        }
        print(json.dumps(summary, indent=2, ensure_ascii=False))
        return

    from sentence_transformers import SentenceTransformer

    teacher_providers = resolve_teacher_providers(args.teacher_provider)
    teacher_session = build_teacher_session_config()
    print(
        "Resource profile: "
        f"{args._resource_profile} topology={json.dumps(topology, ensure_ascii=False)} "
        f"torch_threads={args._torch_threads}"
    )
    print(f"Teacher providers: {teacher_providers}")
    print(f"Teacher session: {teacher_session}")

    # Load student
    print(f"Loading student ({args.student_model})...")
    student = SentenceTransformer(args.student_model)
    student.max_seq_length = resolved_max_seq_length(args.max_seq_length)
    print(f"Student max_seq_length: {student.max_seq_length}")

    # Stage 1: Distillation
    model_path = None
    if args.stage in {"all", "distill"}:
        texts = collect_code_texts(
            args.distill_texts,
            distill_input=args.distill_input,
            finetune_input=args.finetune_input,
            seed=args.seed,
        )
        print(f"Collected {len(texts)} code texts for distillation")
        student = stage1_distill(
            student,
            texts,
            args,
            teacher_providers,
        )
        if args.stage == "distill":
            model_path = Path(args.output) / "stage1-model"
            model_path.mkdir(parents=True, exist_ok=True)
            student.save(str(model_path))
            print(f"Stage 1 model saved to {model_path}")

    # Stage 2: MNRL fine-tuning
    if args.stage in {"all", "finetune"}:
        if args.stage == "finetune":
            print("Skipping Stage 1 distillation (--stage finetune)")
        model_path = stage2_finetune(student, args.finetune_input, args)

    # Export ONNX
    if model_path is not None and not args.skip_onnx:
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
