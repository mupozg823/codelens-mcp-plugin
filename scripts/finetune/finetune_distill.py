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
import os
import sys
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
        "--teacher-provider",
        choices=["auto", "cpu", "coreml"],
        default="auto",
        help="Execution provider for ONNX teacher inference",
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
    parser.add_argument("--output", default="")
    parser.add_argument(
        "--evaluation-steps",
        type=int,
        default=0,
        help="Run evaluator every N optimization steps (0 = epoch end only)",
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
    threads = requested_threads if requested_threads > 0 else default_torch_threads(profile_name)
    os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")
    os.environ.setdefault("OMP_NUM_THREADS", str(threads))
    if sys.platform == "darwin":
        os.environ.setdefault("VECLIB_MAXIMUM_THREADS", str(threads))

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


def effective_train_batch_size(requested_batch_size: int, profile_name: str) -> int:
    return requested_batch_size


def effective_distill_batch_size(requested_batch_size: int, profile_name: str) -> int:
    return requested_batch_size if requested_batch_size > 0 else 16


def resolve_training_device(device_name: str, *, strict: bool = True) -> str:
    try:
        import torch
    except ModuleNotFoundError:
        if strict:
            raise SystemExit("torch is required to resolve the requested training device")
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
) -> int:
    return recommended_loader_workers(requested_workers, num_examples)


def resolve_teacher_providers(provider_name: str, *, strict: bool = True) -> list[str]:
    try:
        import onnxruntime as ort
    except ModuleNotFoundError:
        if strict:
            raise SystemExit("onnxruntime is required to resolve the requested teacher provider")
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
            raise SystemExit("Requested --teacher-provider coreml, but CoreMLExecutionProvider is unavailable")
        return ["CoreMLExecutionProvider", "CPUExecutionProvider"]
    if sys.platform == "darwin" and "CoreMLExecutionProvider" in available:
        return ["CoreMLExecutionProvider", "CPUExecutionProvider"]
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
) -> Path:
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

    cache_dir = Path(output_dir) / "cache"
    cache_dir.mkdir(parents=True, exist_ok=True)
    return cache_dir / f"teacher-embeddings-{key.hexdigest()[:16]}.npy"


def teacher_embed(model, tokenizer, texts, batch_size=64):
    """Generate embeddings from teacher model."""
    import numpy as np

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


def load_or_compute_teacher_embeddings(
    model,
    tokenizer,
    texts: list[str],
    args,
    teacher_providers: list[str],
):
    import numpy as np

    cache_path = teacher_cache_path(args.output, texts, args.teacher_dir, teacher_providers)
    if cache_path.exists():
        embeddings = np.load(cache_path)
        print(f"  Teacher cache hit: {cache_path}")
        return embeddings

    embeddings = teacher_embed(model, tokenizer, texts)
    np.save(cache_path, embeddings)
    print(f"  Teacher cache saved: {cache_path}")
    return embeddings


def collect_code_texts(n=3000, distill_input: str = "", finetune_input: str = ""):
    """Collect generic texts for teacher alignment without benchmark leakage."""
    if distill_input:
        rows = []
        with open(distill_input, encoding="utf-8") as f:
            for line in f:
                obj = json.loads(line)
                text = obj.get("text", "").strip()
                if text:
                    rows.append(text)
        random.shuffle(rows)
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
        random.shuffle(runtime_texts)
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

    random.shuffle(texts)
    return texts[:n]


def stage1_distill(student, teacher_model, teacher_tokenizer, texts, args, teacher_providers):
    """Align student embeddings with teacher via MSE loss."""
    import torch

    print(
        f"\n=== Stage 1: Distillation ({len(texts)} texts, {args.distill_epochs} epochs) ==="
    )

    # Get teacher embeddings
    print("  Generating teacher embeddings...")
    teacher_embeddings = load_or_compute_teacher_embeddings(
        teacher_model,
        teacher_tokenizer,
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

    target_tensor = torch.tensor(teacher_embeddings, dtype=torch.float32).to(device)

    optimizer = torch.optim.AdamW(student_model.parameters(), lr=2e-5)
    mse_loss = torch.nn.MSELoss()

    for epoch in range(args.distill_epochs):
        total_loss = 0.0
        batches = 0
        distill_batch_size = effective_distill_batch_size(
            args.distill_batch_size or args.batch_size,
            args._resource_profile,
        )
        for i in range(0, len(texts), distill_batch_size):
            batch_texts = texts[i : i + distill_batch_size]
            batch_targets = target_tensor[i : i + distill_batch_size]

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

    del target_tensor
    del teacher_embeddings
    if device.type == "mps":
        torch.mps.empty_cache()
    print("Stage 1 complete.")
    return student


def load_retrieval_pairs(path: str) -> list[tuple[str, str]]:
    pairs = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            obj = json.loads(line.strip())
            query = obj.get("query", "").strip()
            positive = obj.get("positive", "").strip()
            if query and positive:
                pairs.append((query, positive))
    return pairs


def build_retrieval_dataset(pairs: list[tuple[str, str]]):
    from datasets import Dataset

    sentence_0 = [query for query, _ in pairs]
    sentence_1 = [positive for _, positive in pairs]
    return Dataset.from_dict({"sentence_0": sentence_0, "sentence_1": sentence_1})


def build_validation_evaluator(validation_input: str, batch_size: int):
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

    for idx, (query, positive) in enumerate(load_retrieval_pairs(str(path))):
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
    from sentence_transformers.training_args import BatchSamplers, SentenceTransformerTrainingArguments

    print(f"\n=== Stage 2: MNRL Fine-tuning ({args.finetune_epochs} epochs) ===")

    pairs = load_retrieval_pairs(triplets_path)

    print(f"  Loaded {len(pairs)} query-positive pairs")
    train_device = resolve_training_device(args.train_device)
    student = student.to(train_device)
    print(f"  Student device: {train_device}")
    train_dataset = build_retrieval_dataset(pairs)
    train_batch_size = effective_train_batch_size(args.batch_size, args._resource_profile)
    loader_workers = effective_loader_workers(
        args.loader_workers,
        len(pairs),
        args._resource_profile,
    )
    batch_sampler = (
        BatchSamplers.BATCH_SAMPLER
        if args.disable_no_duplicates_loader
        else BatchSamplers.NO_DUPLICATES
    )
    print(
        "  Trainer dataloading: "
        f"batch_sampler={batch_sampler.value} workers={loader_workers} batch_size={train_batch_size}"
    )

    if args.use_cached_mnrl:
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

    steps_per_epoch = max(1, len(train_dataset) // train_batch_size)
    warmup = int(steps_per_epoch * args.finetune_epochs * 0.1)
    evaluator = build_validation_evaluator(args.validation_input, args.batch_size)

    training_args = SentenceTransformerTrainingArguments(
        output_dir=str(Path(args.output) / "checkpoints"),
        per_device_train_batch_size=train_batch_size,
        per_device_eval_batch_size=train_batch_size,
        batch_sampler=batch_sampler,
        num_train_epochs=args.finetune_epochs,
        warmup_steps=warmup,
        learning_rate=1e-5,
        save_strategy="no",
        eval_strategy="steps" if evaluator and args.evaluation_steps > 0 else "no",
        eval_steps=args.evaluation_steps if evaluator and args.evaluation_steps > 0 else None,
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
        evaluator=evaluator,
    )
    trainer.train()
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
    args._torch_threads = configure_process_runtime(args._resource_profile, args.torch_threads)

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
        summary = {
            "finetune_input": args.finetune_input,
            "validation_input": args.validation_input,
            "distill_input": args.distill_input,
            "output": args.output,
            "pipeline_manifest": manifest.get("_manifest_path") if manifest else None,
            "resource_profile_requested": args.resource_profile,
            "resource_profile": args._resource_profile,
            "topology": topology,
            "torch_threads": args._torch_threads,
            "train_device_requested": args.train_device,
            "train_device": resolve_training_device(args.train_device, strict=False),
            "teacher_providers": resolve_teacher_providers(
                args.teacher_provider,
                strict=False,
            ),
            "teacher_session": build_teacher_session_config(),
            "effective_train_batch_size": effective_train_batch_size(
                args.batch_size,
                args._resource_profile,
            ),
            "effective_distill_batch_size": effective_distill_batch_size(
                args.distill_batch_size or args.batch_size,
                args._resource_profile,
            ),
            "train_pairs": len(load_retrieval_pairs(args.finetune_input)),
            "validation_pairs": len(load_retrieval_pairs(args.validation_input))
            if args.validation_input
            else 0,
        }
        print(json.dumps(summary, indent=2, ensure_ascii=False))
        return

    from sentence_transformers import SentenceTransformer

    # Load teacher (CodeSearchNet ONNX)
    print("Loading teacher (CodeSearchNet ONNX)...")
    teacher_model, teacher_tokenizer, teacher_providers, teacher_session = load_teacher(
        args.teacher_dir,
        args.teacher_provider,
    )
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

    # Stage 1: Distillation
    texts = collect_code_texts(
        args.distill_texts,
        distill_input=args.distill_input,
        finetune_input=args.finetune_input,
    )
    print(f"Collected {len(texts)} code texts for distillation")
    student = stage1_distill(
        student,
        teacher_model,
        teacher_tokenizer,
        texts,
        args,
        teacher_providers,
    )

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
