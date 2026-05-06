#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: scripts/package-release-artifact.sh --name NAME --binary PATH --binary-name NAME --archive-ext EXT [options]

Builds a standard CodeLens release archive from an already-built binary and
bundled model assets. The package intentionally contains only:
  - codelens-mcp / codelens-mcp.exe
  - models/codesearch/*

Options:
  --name NAME          Release target name, e.g. linux-x86_64 or windows-x86_64.
  --binary PATH        Built binary to copy into the package.
  --binary-name NAME   Binary name inside the archive.
  --archive-ext EXT    Archive extension: .tar.gz or .zip.
  --output-dir DIR     Directory for the final archive. Defaults to current dir.
  --models-dir DIR     Model assets root. Defaults to CODELENS_RELEASE_MODELS_DIR
                       or crates/codelens-engine/models.
  --target TARGET      Optional build target label for logs.
  -h, --help           Show this help.
EOF
}

name=""
binary_path=""
binary_name=""
archive_ext=""
output_dir="."
models_dir="${CODELENS_RELEASE_MODELS_DIR:-crates/codelens-engine/models}"
target=""

while (($# > 0)); do
	case "$1" in
		-h|--help)
			usage
			exit 0
			;;
		--name)
			shift
			name="${1:-}"
			;;
		--binary)
			shift
			binary_path="${1:-}"
			;;
		--binary-name)
			shift
			binary_name="${1:-}"
			;;
		--archive-ext)
			shift
			archive_ext="${1:-}"
			;;
		--output-dir)
			shift
			output_dir="${1:-}"
			;;
		--models-dir)
			shift
			models_dir="${1:-}"
			;;
		--target)
			shift
			target="${1:-}"
			;;
		--*)
			echo "unknown option: $1" >&2
			usage >&2
			exit 2
			;;
		*)
			echo "unexpected argument: $1" >&2
			usage >&2
			exit 2
			;;
	esac
	shift || true
done

if [[ -z "$name" || -z "$binary_path" || -z "$binary_name" || -z "$archive_ext" ]]; then
	usage >&2
	exit 2
fi
if [[ "$archive_ext" != ".tar.gz" && "$archive_ext" != ".zip" ]]; then
	echo "unsupported archive extension: $archive_ext" >&2
	exit 2
fi
if [[ ! -f "$binary_path" ]]; then
	echo "binary not found: $binary_path" >&2
	exit 1
fi
if [[ ! -d "$models_dir/codesearch" ]]; then
	echo "models directory not found: $models_dir/codesearch" >&2
	exit 1
fi

mkdir -p "$output_dir"
archive="$output_dir/codelens-mcp-${name}${archive_ext}"
package_dir="$(mktemp -d "${TMPDIR:-/tmp}/codelens-release-package.XXXXXX")"
cleanup() {
	rm -rf "$package_dir"
}
trap cleanup EXIT

mkdir -p "$package_dir/models/codesearch"
cp "$binary_path" "$package_dir/$binary_name"

for asset in model.onnx tokenizer.json config.json special_tokens_map.json tokenizer_config.json; do
	cp -L "$models_dir/codesearch/$asset" "$package_dir/models/codesearch/$asset"
done

python3 scripts/verify-model-assets.py --root "$package_dir" \
	--write-manifest "$package_dir/models/codesearch/model-manifest.json" \
	--check \
	--quiet

rm -f "$archive"
case "$archive_ext" in
	.tar.gz)
		COPYFILE_DISABLE=1 tar czf "$archive" -C "$package_dir" "$binary_name" models
		;;
	.zip)
		python3 - "$archive" "$package_dir" <<'PY'
import sys
import zipfile
from pathlib import Path

archive = Path(sys.argv[1])
root = Path(sys.argv[2])
with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as zf:
    for path in sorted(root.rglob("*")):
        if path.is_file():
            zf.write(path, path.relative_to(root).as_posix())
PY
		;;
esac

python3 scripts/verify-model-assets.py --archive "$archive" --check --quiet

if [[ -n "$target" ]]; then
	echo "packaged release artifact for $target: $archive"
else
	echo "packaged release artifact: $archive"
fi
