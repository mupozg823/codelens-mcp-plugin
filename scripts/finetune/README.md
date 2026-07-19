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
  --stdout summary \
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

## Dense Hard Negatives (opt-in)

Mine dense hard negatives, then wire them into training as explicit MNRL
triplets. The mining script defers all ML imports so `--dry-run` only emits a
plan JSON (CI-safe):

```bash
# Plan only (no ML deps imported):
python3 scripts/finetune/mine_hard_negatives.py --dry-run

# Real mining in an ML environment:
python3 scripts/finetune/mine_hard_negatives.py \
  --train-data scripts/finetune/curated_1k_pairs.jsonl \
  --output scripts/finetune/hard_negatives.jsonl \
  --num-negatives 5
```

Defaults use `sampling_strategy='top'`, `num_negatives=5`, and
`relative_margin=0.05` (the false-negative guard). Feed the output to training
with `--hard-negatives`; each mined negative becomes a `[query, positive,
negative]` triplet run as a second MNRL objective alongside the in-batch pairs.
The dry-run `training-plan.json` gains a `hard_negatives` block (used/path/rows/
params); when the flag is omitted it reports `used: false, loss_input:
in_batch_only`, leaving every existing plan field unchanged.

```bash
python3 scripts/finetune/train_codelens_lora.py --dry-run \
  --hard-negatives scripts/finetune/hard_negatives.jsonl
```

## LLM Synthetic NL Queries (opt-in)

`build_nl_augmentation.py --synth-queries` adds an E5-Mistral 2-step synthesis
stage (brainstorm retrieval task types, then a query per type) behind a
pluggable generator. The default `stub` backend performs no model/network call;
it writes a plan of `(snippet, prompt, expected schema)` records for a later,
user-approved generation pass:

```bash
python3 scripts/finetune/build_nl_augmentation.py --synth-queries --dry-run \
  --synth-input scripts/finetune/synthetic_nl_pairs.jsonl \
  --synth-output scripts/finetune/output/nl-synth-plan.jsonl
```

## Semantic Near-Duplicate Audit

`contamination_audit.py` adds an embedding-cosine near-duplicate stage on top of
the exact/normalized/copied detectors. It reports a `semantic_near_duplicates`
section and, when it runs, fails on any training↔benchmark pair at/above
`--semantic-threshold` (default `0.9`). If sentence-transformers or the model is
unavailable the stage degrades to `SKIPPED` with a reason, so the fail-closed
gates still hold in dependency-free CI.
