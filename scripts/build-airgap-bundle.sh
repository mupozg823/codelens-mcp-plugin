#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: scripts/build-airgap-bundle.sh --archive PATH --sbom PATH [options]

Build a self-contained air-gapped bundle from a released CodeLens archive.

Required:
  --archive PATH         Path to packaged release archive (tar.gz or zip)
  --sbom PATH            Path to CycloneDX SBOM JSON for the same artifact

Options:
  --platform NAME        Platform label for output naming
                         Default: derived from archive filename
  --version VERSION      Version string included in bundle manifest
                         Default: unknown
  --models-dir PATH      Directory containing model assets
                         Default: crates/codelens-engine/models
  --output PATH          Output tar.gz path
                         Default: ./codelens-mcp-airgap-<platform>.tar.gz
  -h, --help             Show help
EOF
}

archive_path=""
sbom_path=""
platform=""
version="unknown"
models_dir="crates/codelens-engine/models"
output_path=""

while (($# > 0)); do
	case "$1" in
		--archive)
			shift
			archive_path="${1:-}"
			;;
		--sbom)
			shift
			sbom_path="${1:-}"
			;;
		--platform)
			shift
			platform="${1:-}"
			;;
		--version)
			shift
			version="${1:-}"
			;;
		--models-dir)
			shift
			models_dir="${1:-}"
			;;
		--output)
			shift
			output_path="${1:-}"
			;;
		-h|--help)
			usage
			exit 0
			;;
		*)
			echo "unknown argument: $1" >&2
			usage >&2
			exit 2
			;;
	esac
	shift || true
done

if [[ -z "$archive_path" || -z "$sbom_path" ]]; then
	echo "--archive and --sbom are required" >&2
	usage >&2
	exit 2
fi

if [[ ! -f "$archive_path" ]]; then
	echo "archive not found: $archive_path" >&2
	exit 1
fi

if [[ ! -f "$sbom_path" ]]; then
	echo "sbom not found: $sbom_path" >&2
	exit 1
fi

for required in \
	"$models_dir/codesearch/model.onnx" \
	"$models_dir/codesearch/tokenizer.json" \
	"$models_dir/codesearch/config.json" \
	"$models_dir/codesearch/special_tokens_map.json" \
	"$models_dir/codesearch/tokenizer_config.json"; do
	if [[ ! -f "$required" ]]; then
		echo "models directory missing required payload file: $required" >&2
		exit 1
	fi
done

if [[ -z "$platform" ]]; then
	base="$(basename "$archive_path")"
	platform="${base#codelens-mcp-}"
	platform="${platform%.tar.gz}"
	platform="${platform%.zip}"
fi

if [[ -z "$output_path" ]]; then
	output_path="codelens-mcp-airgap-${platform}.tar.gz"
fi

bundle_root="codelens-mcp-airgap-${platform}"
tmpdir="$(mktemp -d)"
cleanup() {
	rm -rf "$tmpdir"
}
trap cleanup EXIT

bundle_dir="$tmpdir/$bundle_root"
mkdir -p "$bundle_dir/models" "$bundle_dir/sbom" "$bundle_dir/examples"

case "$archive_path" in
	*.tar.gz)
		tar -xzf "$archive_path" -C "$bundle_dir"
		;;
	*.zip)
		if command -v unzip >/dev/null 2>&1; then
			unzip -q "$archive_path" -d "$bundle_dir"
		elif command -v 7z >/dev/null 2>&1; then
			7z x -y "-o$bundle_dir" "$archive_path" >/dev/null
		else
			echo "missing archive extractor for zip input: need unzip or 7z" >&2
			exit 1
		fi
		;;
	*)
		echo "unsupported archive format: $archive_path" >&2
		exit 1
		;;
esac

rm -rf "$bundle_dir/models/codesearch"
cp -R "$models_dir/codesearch" "$bundle_dir/models/"
python3 scripts/verify-model-assets.py "$bundle_dir/models" \
	--write-manifest "$bundle_dir/models/codesearch/model-manifest.json" \
	--quiet
cp "$sbom_path" "$bundle_dir/sbom/"
cp LICENSE "$bundle_dir/"

cat >"$bundle_dir/examples/mcp-stdio.json" <<'EOF'
{
  "mcpServers": {
    "codelens": {
      "command": "./codelens-mcp",
      "args": [".", "--profile", "builder-minimal"]
    }
  }
}
EOF

cat >"$bundle_dir/examples/mcp-http.json" <<'EOF'
{
  "mcpServers": {
    "codelens": {
      "type": "http",
      "url": "http://127.0.0.1:7837/mcp"
    }
  }
}
EOF

cat >"$bundle_dir/examples/launch-readonly-http.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
exec ./codelens-mcp /workspace --transport http --profile reviewer-graph --daemon-mode read-only --port 7837
EOF

cat >"$bundle_dir/examples/launch-mutation-http.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
exec ./codelens-mcp /workspace --transport http --profile refactor-full --daemon-mode mutation-enabled --port 7838
EOF

chmod +x \
	"$bundle_dir/examples/launch-readonly-http.sh" \
	"$bundle_dir/examples/launch-mutation-http.sh"

cat >"$bundle_dir/bundle-manifest.json" <<EOF
{
  "bundle_name": "${bundle_root}",
  "version": "${version}",
  "platform": "${platform}",
  "binary_archive": "$(basename "$archive_path")",
  "sbom_file": "$(basename "$sbom_path")",
  "models_dir": "models/codesearch",
  "entrypoint": "./codelens-mcp",
  "http_ports": [7837, 7838]
}
EOF

cat >"$bundle_dir/AIRGAP-BUNDLE.md" <<EOF
# CodeLens MCP Air-Gapped Bundle

This bundle is intended for offline or tightly controlled deployments.

Contents:

- \`./codelens-mcp\` release binary
- \`./models/codesearch/\` bundled semantic model assets
- \`./sbom/$(basename "$sbom_path")\` CycloneDX SBOM for the bundled binary
- \`./examples/\` example MCP configs and daemon launch scripts
- \`./checksums-sha256.txt\` checksums for bundle contents

Run in place:

\`\`\`bash
./codelens-mcp /workspace
\`\`\`

HTTP daemon:

\`\`\`bash
./examples/launch-readonly-http.sh
\`\`\`

Model discovery:

- the binary resolves \`./models/codesearch\` next to itself automatically
- alternatively set \`CODELENS_MODEL_DIR\` to the parent \`models/\` directory
EOF

checksum_tool=""
if command -v sha256sum >/dev/null 2>&1; then
	checksum_tool="sha256sum"
elif command -v shasum >/dev/null 2>&1; then
	checksum_tool="shasum -a 256"
else
	echo "missing checksum tool: need sha256sum or shasum" >&2
	exit 1
fi

(
	cd "$bundle_dir"
	find . -type f ! -name checksums-sha256.txt | LC_ALL=C sort | while IFS= read -r file; do
		file="${file#./}"
		# shellcheck disable=SC2086
		$checksum_tool "$file"
	done > checksums-sha256.txt
)

tar -czf "$output_path" -C "$tmpdir" "$bundle_root"
echo "created air-gapped bundle: $output_path"
