# CodeLens Embedding Fine-Tuning

This directory contains the training and promotion-gate pipeline for semantic
retrieval models used by CodeLens.

## CodeLens LoRA Path

Use `train_codelens_lora.py` for the current adapter path. It trains a LoRA
adapter with `MultipleNegativesRankingLoss`, merges the adapter into the base
SentenceTransformer checkpoint, exports ONNX, and dynamically quantizes the
runtime model to INT8.

Start with a dry run:

```bash
python3 scripts/finetune/train_codelens_lora.py --dry-run
```

The dry run writes `scripts/finetune/output/codelens-lora/training-plan.json`
without importing PyTorch or ONNX dependencies. This is the CI-safe contract
check for dataset shape, adapter settings, runtime manifest fields, and the
promotion-gate command.

Run training in an ML environment:

```bash
python3 scripts/finetune/train_codelens_lora.py \
  --base-model sentence-transformers/all-MiniLM-L12-v2 \
  --train-data scripts/finetune/pipelines/v12-sanitized/train.jsonl \
  --validation-data scripts/finetune/pipelines/v12-sanitized/validation.jsonl \
  --output-dir /tmp/codelens-lora
```

Required Python packages for non-dry-run training:

```bash
python3 -m pip install sentence-transformers peft torch "optimum[onnxruntime]" transformers onnxruntime
```

After training, run the emitted promotion gate before using the model:

```bash
python3 scripts/finetune/promotion_gate.py \
  --candidate-onnx-dir /tmp/codelens-lora/onnx \
  --candidate-label MiniLM-L12-CodeLens-LoRA-INT8
```

Compare all existing local ONNX models with the same benchmark settings:

```bash
python3 benchmarks/existing-model-bakeoff.py . \
  --binary target/debug/codelens-mcp \
  --output-dir /tmp/codelens-existing-model-bakeoff
```

The bakeoff uses isolated project copies by default so model comparisons do not
reuse a `.codelens` embedding index produced by another model. Use
`--reuse-project-index` only for quick smoke checks.

Prepare a bundled-teacher sanity candidate and run the promotion gate:

```bash
python3 scripts/finetune/prepare_bundled_teacher_candidate.py \
  --output-dir /tmp/codelens-bundled-teacher-candidate \
  --label bundled-teacher-noop

python3 scripts/finetune/promotion_gate.py \
  --project . \
  --binary target/debug/codelens-mcp \
  --candidate-onnx-dir /tmp/codelens-bundled-teacher-candidate/onnx \
  --candidate-label bundled-teacher-noop \
  --candidate-manifest /tmp/codelens-bundled-teacher-candidate/onnx/model-manifest.json \
  --output-dir /tmp/codelens-promotion-gate-bundled-teacher
```

This candidate intentionally copies the bundled model without LoRA changes. It
validates the teacher/baseline identity, candidate staging, contamination audit,
and promotion-gate plumbing before spending compute on a trained LoRA artifact.

For local runtime testing, point CodeLens at the generated ONNX directory:

```bash
CODELENS_MODEL_DIR=/tmp/codelens-lora/onnx \
CODELENS_EMBED_MODEL=MiniLM-L12-CodeLens-LoRA-INT8 \
python3 benchmarks/embedding-quality.py . \
  --ranked-context-max-tokens 50000
```

For the local dual-daemon setup, `scripts/install-http-daemons-launchd.sh`
builds the HTTP daemon with `http,semantic` by default and writes
`CODELENS_MODEL_DIR` into the generated plists when a model directory is
available. Use `--model-dir /tmp/codelens-lora/onnx` to dogfood a candidate
adapter, or `--no-semantic` for an HTTP-only daemon.

Do not promote a LoRA candidate only because semantic search improves. The
candidate must pass hybrid retrieval, external retrieval, role retrieval, and
session replay gates without regressing the current baseline.
