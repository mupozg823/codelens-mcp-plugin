#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: scripts/verify-release-artifacts.sh [bundle_dir] [--checksums PATH] [--require-targets LIST]

Verifies CodeLens release artifacts against checksums-sha256.txt and validates
their archive structure.

Arguments:
  bundle_dir              Directory containing release archives and checksums file.
                          Defaults to current directory.

Options:
  --checksums PATH        Override checksums file path.
  --require-targets LIST  Comma-separated expected targets.
                          Default: darwin-arm64,linux-x86_64,windows-x86_64
  -h, --help              Show this help.

Examples:
  scripts/verify-release-artifacts.sh ./dist
  scripts/verify-release-artifacts.sh . --require-targets darwin-arm64,linux-x86_64
EOF
}

bundle_dir="."
checksums_path=""
require_targets="darwin-arm64,linux-x86_64,windows-x86_64"

while (($# > 0)); do
	case "$1" in
		-h|--help)
			usage
			exit 0
			;;
		--checksums)
			shift
			checksums_path="${1:-}"
			;;
		--require-targets)
			shift
			require_targets="${1:-}"
			;;
		--*)
			echo "unknown option: $1" >&2
			usage >&2
			exit 2
			;;
		*)
			bundle_dir="$1"
			;;
	esac
	shift || true
done

if [[ ! -d "$bundle_dir" ]]; then
	echo "bundle_dir does not exist: $bundle_dir" >&2
	exit 1
fi

if [[ -z "$checksums_path" ]]; then
	checksums_path="$bundle_dir/checksums-sha256.txt"
fi

if [[ ! -f "$checksums_path" ]]; then
	echo "checksums file not found: $checksums_path" >&2
	exit 1
fi

checksum_tool=""
checksum_args=()
if command -v sha256sum >/dev/null 2>&1; then
	checksum_tool="sha256sum"
	checksum_args=(-c)
elif command -v shasum >/dev/null 2>&1; then
	checksum_tool="shasum"
	checksum_args=(-a 256 -c)
else
	echo "missing checksum tool: need sha256sum or shasum" >&2
	exit 1
fi

assets=()
while IFS= read -r asset; do
	[[ -z "$asset" ]] && continue
	assets+=("$asset")
done < <(awk '{print $2}' "$checksums_path")
if [[ ${#assets[@]} -eq 0 ]]; then
	echo "checksums file is empty: $checksums_path" >&2
	exit 1
fi

duplicate_assets="$(printf '%s\n' "${assets[@]}" | LC_ALL=C sort | uniq -d)"
if [[ -n "$duplicate_assets" ]]; then
	echo "checksums file contains duplicate artifact entries:" >&2
	printf '  %s\n' "$duplicate_assets" >&2
	exit 1
fi

for asset in "${assets[@]}"; do
	if [[ ! -f "$bundle_dir/$asset" ]]; then
		echo "missing artifact referenced by checksums: $bundle_dir/$asset" >&2
		exit 1
	fi
done

required_missing=0
IFS=',' read -r -a required_targets <<< "$require_targets"
for target in "${required_targets[@]}"; do
	target="$(echo "$target" | xargs)"
	[[ -z "$target" ]] && continue
	if ! printf '%s\n' "${assets[@]}" | grep -Eq "^codelens-mcp-${target}\.(tar\.gz|zip)$"; then
		echo "missing expected target artifact: $target" >&2
		required_missing=1
	fi
done
if [[ $required_missing -ne 0 ]]; then
	exit 1
fi

(
	cd "$(dirname "$checksums_path")"
	"$checksum_tool" "${checksum_args[@]}" "$(basename "$checksums_path")"
)

verify_tar_structure() {
	local archive="$1"
	local -a entries
	while IFS= read -r entry; do
		[[ -z "$entry" ]] && continue
		entries+=("$entry")
	done < <(tar -tzf "$archive")
	verify_standard_payload_entries "$archive" "codelens-mcp" "${entries[@]}"
}

verify_standard_payload_entries() {
	local archive="$1"
	local binary_name="$2"
	shift 2
	local -a entries=("$@")
	for required in \
		"$binary_name" \
		"models/codesearch/model.onnx" \
		"models/codesearch/tokenizer.json" \
		"models/codesearch/config.json" \
		"models/codesearch/special_tokens_map.json" \
		"models/codesearch/tokenizer_config.json" \
		"models/codesearch/model-manifest.json" \
		"adapters/roslyn-workspace-service/CodeLens.Roslyn.WorkspaceService.dll"; do
		if ! printf '%s\n' "${entries[@]}" | grep -Fxq "$required"; then
			echo "standard release archive missing required file $required in $archive" >&2
			return 1
		fi
	done
}

verify_airgap_bundle() {
	local archive="$1"
	local tmpdir bundle_root bundle_dir
	tmpdir="$(mktemp -d)"
	trap 'rm -rf "$tmpdir"' RETURN

	tar -xzf "$archive" -C "$tmpdir"
	bundle_root="$(find "$tmpdir" -mindepth 1 -maxdepth 1 -type d | head -1)"
	if [[ -z "$bundle_root" ]]; then
		echo "airgap bundle missing top-level directory: $archive" >&2
		return 1
	fi
	bundle_dir="$bundle_root"

	for required in \
		"codelens-mcp" \
		"models/codesearch/model.onnx" \
		"models/codesearch/tokenizer.json" \
		"models/codesearch/config.json" \
		"models/codesearch/special_tokens_map.json" \
		"models/codesearch/tokenizer_config.json" \
		"models/codesearch/model-manifest.json" \
		"adapters/roslyn-workspace-service/CodeLens.Roslyn.WorkspaceService.dll" \
		"checksums-sha256.txt" \
		"AIRGAP-BUNDLE.md" \
		"bundle-manifest.json" \
		"examples/mcp-stdio.json" \
		"examples/mcp-http.json"; do
		if [[ ! -f "$bundle_dir/$required" ]]; then
			echo "airgap bundle missing required file $required in $archive" >&2
			return 1
		fi
	done

	(
		cd "$bundle_dir"
		"$checksum_tool" "${checksum_args[@]}" checksums-sha256.txt >/dev/null
	)
}

verify_zip_structure() {
	local archive="$1"
	local -a entries
	if command -v unzip >/dev/null 2>&1; then
		while IFS= read -r entry; do
			[[ -z "$entry" ]] && continue
			entries+=("$entry")
		done < <(unzip -Z1 "$archive")
	elif command -v 7z >/dev/null 2>&1; then
		while IFS= read -r entry; do
			[[ -z "$entry" ]] && continue
			entries+=("$entry")
		done < <(7z l -ba "$archive" | awk 'NF >= 6 {print $NF}')
	else
		echo "missing archive tool for zip validation: need unzip or 7z" >&2
		return 1
	fi
	verify_standard_payload_entries "$archive" "codelens-mcp.exe" "${entries[@]}"
}

verify_sbom_structure() {
	local sbom="$1"
	python3 - "$sbom" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
obj = json.loads(path.read_text())
if obj.get("bomFormat") != "CycloneDX":
    raise SystemExit(f"unexpected bomFormat in {path}: {obj.get('bomFormat')!r}")
if not obj.get("specVersion"):
    raise SystemExit(f"missing specVersion in {path}")
component = obj.get("metadata", {}).get("component", {})
if component.get("name") != "codelens-mcp":
    raise SystemExit(f"unexpected metadata.component.name in {path}: {component.get('name')!r}")
PY
}

verify_release_manifest() {
	local manifest="$1"
	python3 - "$manifest" "$checksums_path" <<'PY'
import json
import re
import sys
from pathlib import Path

manifest_path = Path(sys.argv[1])
checksums_path = Path(sys.argv[2])
obj = json.loads(manifest_path.read_text())

if obj.get("schema_version") != "codelens-release-manifest-v1":
    raise SystemExit(
        f"unexpected schema_version in {manifest_path}: {obj.get('schema_version')!r}"
    )
if not obj.get("repository"):
    raise SystemExit(f"missing repository in {manifest_path}")
if not obj.get("tag"):
    raise SystemExit(f"missing tag in {manifest_path}")
if not obj.get("version"):
    raise SystemExit(f"missing version in {manifest_path}")

assets = obj.get("assets")
if not isinstance(assets, list) or not assets:
    raise SystemExit(f"manifest assets must be a non-empty list in {manifest_path}")

checksum_entries = {}
for raw_line in checksums_path.read_text().splitlines():
    line = raw_line.strip()
    if not line:
        continue
    checksum, name = line.split(maxsplit=1)
    checksum_entries[name] = checksum

payload_patterns = [
    re.compile(r"^codelens-mcp-airgap-.+\.tar\.gz$"),
    re.compile(r"^codelens-mcp-.+\.tar\.gz$"),
    re.compile(r"^codelens-mcp-.+\.zip$"),
    re.compile(r"^codelens-mcp-.+\.cdx\.json$"),
]

def is_payload_asset(name: str) -> bool:
    return any(pattern.match(name) for pattern in payload_patterns)

payload_entries = {
    name: checksum
    for name, checksum in checksum_entries.items()
    if name != manifest_path.name and is_payload_asset(name)
}
manifest_entries = {}
for asset in assets:
    if not isinstance(asset, dict):
        raise SystemExit(f"manifest asset must be an object in {manifest_path}")
    name = asset.get("name")
    sha256 = asset.get("sha256")
    kind = asset.get("kind")
    target = asset.get("target")
    download_url = asset.get("download_url")
    if not all(isinstance(value, str) and value for value in (name, sha256, kind, target, download_url)):
        raise SystemExit(f"manifest asset missing required strings in {manifest_path}: {asset!r}")
    expected_sha = checksum_entries.get(name)
    if expected_sha != sha256:
        raise SystemExit(
            f"manifest checksum mismatch for {name} in {manifest_path}: {sha256!r} != {expected_sha!r}"
        )
    if name in manifest_entries:
        raise SystemExit(f"duplicate manifest asset entry for {name} in {manifest_path}")
    manifest_entries[name] = sha256

if set(manifest_entries) != set(payload_entries):
    missing = sorted(set(payload_entries) - set(manifest_entries))
    extra = sorted(set(manifest_entries) - set(payload_entries))
    raise SystemExit(
        f"manifest asset set mismatch in {manifest_path}: missing={missing!r} extra={extra!r}"
    )
PY
}

verify_signature_file() {
	local signature="$1"
	local base="${signature%.sig}"
	is_signable_asset "$(basename "$base")" || {
		echo "unexpected signature sidecar without signable payload: $signature" >&2
		return 1
	}
	if [[ ! -s "$signature" ]]; then
		echo "signature file is empty: $signature" >&2
		return 1
	fi
}

verify_certificate_file() {
	local cert="$1"
	local base="${cert%.pem}"
	is_signable_asset "$(basename "$base")" || {
		echo "unexpected certificate sidecar without signable payload: $cert" >&2
		return 1
	}
	[[ -s "$cert" ]] || {
		echo "certificate file is empty: $cert" >&2
		return 1
	}
}

is_signable_asset() {
	case "$1" in
		codelens-mcp-airgap-*.tar.gz|codelens-mcp-*.tar.gz|codelens-mcp-*.zip|codelens-mcp-*.cdx.json|release-manifest.json)
			return 0
			;;
		*)
			return 1
			;;
	esac
}

for asset in "${assets[@]}"; do
	if is_signable_asset "$asset"; then
		if [[ ! -f "$bundle_dir/$asset.sig" ]]; then
			echo "missing signature sidecar for $asset: $bundle_dir/$asset.sig" >&2
			exit 1
		fi
		if [[ ! -f "$bundle_dir/$asset.pem" ]]; then
			echo "missing certificate sidecar for $asset: $bundle_dir/$asset.pem" >&2
			exit 1
		fi
	fi
done

for asset in "${assets[@]}"; do
	case "$asset" in
		codelens-mcp-airgap-*.tar.gz)
			verify_airgap_bundle "$bundle_dir/$asset"
			;;
		*.tar.gz)
			verify_tar_structure "$bundle_dir/$asset"
			;;
		*.zip)
			verify_zip_structure "$bundle_dir/$asset"
			;;
		*.cdx.json)
			verify_sbom_structure "$bundle_dir/$asset"
			;;
		release-manifest.json)
			verify_release_manifest "$bundle_dir/$asset"
			;;
		*.sig)
			verify_signature_file "$bundle_dir/$asset"
			;;
		*.pem)
			verify_certificate_file "$bundle_dir/$asset"
			;;
		*)
			echo "unexpected release artifact type in checksums file: $asset" >&2
			exit 1
			;;
	esac
done

echo "verified release bundle:"
echo "  bundle_dir: $bundle_dir"
echo "  checksums:  $(basename "$checksums_path")"
echo "  assets:     ${#assets[@]}"
printf '  targets:    %s\n' "$require_targets"
