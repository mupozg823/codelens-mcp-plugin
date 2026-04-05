#!/usr/bin/env bash
# Compare baseline vs fine-tuned embedding model quality.
#
# Usage:
#   ./scripts/finetune/compare_models.sh /path/to/finetuned/onnx
#
# Prerequisites:
#   - cargo build --release
#   - Fine-tuned model in $1/codesearch/ with model.onnx + tokenizer files

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BIN="$ROOT/target/release/codelens-mcp"
BENCH="$ROOT/benchmarks/embedding-quality.py"

if [ ! -f "$BIN" ]; then
	echo "Build first: cargo build --release"
	exit 1
fi

FINETUNED_DIR="${1:-}"
if [ -z "$FINETUNED_DIR" ]; then
	echo "Usage: $0 /path/to/finetuned/model/dir"
	echo "  The dir should contain codesearch/model.onnx + tokenizer files"
	exit 1
fi

echo "=== Baseline (CodeSearchNet-INT8) ==="
python3 "$BENCH" "$ROOT" \
	--binary "$BIN" \
	--output /tmp/codelens-baseline.json \
	--markdown-output /tmp/codelens-baseline.md

echo ""
echo "=== Fine-tuned ==="
CODELENS_MODEL_DIR="$FINETUNED_DIR" \
	CODELENS_EMBED_MODEL="finetuned-MiniLM-L12" \
	python3 "$BENCH" "$ROOT" \
	--binary "$BIN" \
	--output /tmp/codelens-finetuned.json \
	--markdown-output /tmp/codelens-finetuned.md

echo ""
echo "=== Comparison ==="
python3 -c "
import json

with open('/tmp/codelens-baseline.json') as f:
    base = json.load(f)
with open('/tmp/codelens-finetuned.json') as f:
    fine = json.load(f)

print(f'{\"Method\":<35} {\"Base MRR\":>9} {\"Fine MRR\":>9} {\"Delta\":>8}')
print('-' * 65)
for b, d in zip(base['methods'], fine['methods']):
    name = b['method']
    delta = d['mrr'] - b['mrr']
    sign = '+' if delta >= 0 else ''
    print(f'{name:<35} {b[\"mrr\"]:>9.3f} {d[\"mrr\"]:>9.3f} {sign}{delta:>7.3f}')

print()
print('By query type (hybrid):')
bh = [m for m in base['methods'] if m['method'] == 'get_ranked_context'][0]
fh = [m for m in fine['methods'] if m['method'] == 'get_ranked_context'][0]
for qt in ['identifier', 'natural_language', 'short_phrase']:
    bq = bh['by_query_type'].get(qt, {})
    fq = fh['by_query_type'].get(qt, {})
    if bq and fq:
        delta = fq['mrr'] - bq['mrr']
        sign = '+' if delta >= 0 else ''
        print(f'  {qt:<25} {bq[\"mrr\"]:>9.3f} {fq[\"mrr\"]:>9.3f} {sign}{delta:>7.3f}')
"
