#!/usr/bin/env bash
set -euo pipefail

repo="${GITHUB_REPOSITORY:-mupozg823/codelens-mcp-plugin}"
asset_tag="${CODELENS_RELEASE_MODEL_ASSET_TAG:-model-assets-codesearch-v1}"
asset_name="${CODELENS_RELEASE_MODEL_ASSET_NAME:-codelens-codesearch-model.tar.gz}"
asset_url="${CODELENS_RELEASE_MODEL_ASSET_URL:-https://github.com/${repo}/releases/download/${asset_tag}/${asset_name}}"
asset_path="${CODELENS_RELEASE_MODEL_ASSET_PATH:-}"
models_root="${CODELENS_RELEASE_MODELS_DIR:-crates/codelens-engine/models}"
expected_model_sha="${CODELENS_RELEASE_MODEL_ONNX_SHA256:-ef1d1e9cfa72e4929f5b9faea17d8704b23a2530aadebf0d464a4cc7750920b3}"
required_assets=(
	model.onnx
	tokenizer.json
	config.json
	special_tokens_map.json
	tokenizer_config.json
)

sha256_file() {
	python3 - "$1" <<'PY'
import hashlib
import sys
from pathlib import Path

digest = hashlib.sha256()
with Path(sys.argv[1]).open("rb") as handle:
    for chunk in iter(lambda: handle.read(1 << 20), b""):
        digest.update(chunk)
print(digest.hexdigest())
PY
}

verify_staged_model() {
	local model_file="$models_root/codesearch/model.onnx"
	if [[ ! -f "$model_file" ]]; then
		return 1
	fi
	local actual_sha
	actual_sha="$(sha256_file "$model_file")"
	if [[ "$actual_sha" != "$expected_model_sha" ]]; then
		echo "staged model sha mismatch: expected $expected_model_sha, got $actual_sha" >&2
		return 1
	fi
	python3 scripts/verify-model-assets.py --root "$models_root" --check --quiet
}

if verify_staged_model; then
	echo "release model assets already staged under $models_root/codesearch"
	exit 0
fi

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/codelens-release-model.XXXXXX")"
cleanup() {
	rm -rf "$tmp_dir"
}
trap cleanup EXIT

archive="$tmp_dir/$asset_name"
if [[ -n "$asset_path" ]]; then
	cp "$asset_path" "$archive"
else
	curl -fsSL "$asset_url" -o "$archive"
fi

extract_dir="$tmp_dir/extract"
mkdir -p "$extract_dir"
tar -xzf "$archive" -C "$extract_dir"

source_dir=""
for candidate in "$extract_dir/models/codesearch" "$extract_dir/codesearch"; do
	if [[ -d "$candidate" ]]; then
		source_dir="$candidate"
		break
	fi
done

if [[ -z "$source_dir" ]]; then
	echo "model asset archive must contain codesearch/ or models/codesearch/" >&2
	exit 1
fi

actual_sha="$(sha256_file "$source_dir/model.onnx")"
if [[ "$actual_sha" != "$expected_model_sha" ]]; then
	echo "downloaded model sha mismatch: expected $expected_model_sha, got $actual_sha" >&2
	exit 1
fi

mkdir -p "$models_root/codesearch"
for asset in "${required_assets[@]}"; do
	cp -L "$source_dir/$asset" "$models_root/codesearch/$asset"
done

python3 scripts/verify-model-assets.py --root "$models_root" --check --quiet
echo "staged release model assets under $models_root/codesearch"
