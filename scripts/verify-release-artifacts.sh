#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: scripts/verify-release-artifacts.sh [bundle_dir] [--manifest PATH] [--checksums PATH] [--require-targets LIST]

Verifies CodeLens release artifacts using release-manifest.json as the
authoritative payload inventory and validates their archive structure.

Arguments:
  bundle_dir              Directory containing release archives and manifest.
                          Defaults to current directory.

Options:
  --manifest PATH         Override release manifest path.
  --checksums PATH        Override supplemental checksums file path.
                          When omitted, checksums-sha256.txt is used if present.
  --require-targets LIST  Comma-separated expected targets.
                          Default: darwin-arm64,linux-x86_64,windows-x86_64
  --require-bundles       Require `*.sigstore.json` bundle sidecars for every
                          signable payload and for `checksums-sha256.txt`.
  --verify-bundles-with-cosign
                          Verify mirrored `*.sigstore.json` bundles with
                          `cosign verify-blob` using the configured signer
                          identity and OIDC issuer.
  -h, --help              Show this help.

Examples:
  scripts/verify-release-artifacts.sh ./dist
  scripts/verify-release-artifacts.sh . --require-targets darwin-arm64,linux-x86_64
EOF
}

bundle_dir="."
manifest_path=""
checksums_path=""
checksums_explicit=0
require_targets="darwin-arm64,linux-x86_64,windows-x86_64"
require_bundles=0
verify_bundles_with_cosign=0
certificate_identity_regexp="${CODELENS_CERT_IDENTITY_REGEXP:-https://github.com/mupozg823/codelens-mcp-plugin/.github/workflows/release.yml@refs/tags/.*}"
certificate_oidc_issuer="${CODELENS_CERT_OIDC_ISSUER:-https://token.actions.githubusercontent.com}"

while (($# > 0)); do
	case "$1" in
		-h|--help)
			usage
			exit 0
			;;
		--checksums)
			shift
			checksums_path="${1:-}"
			checksums_explicit=1
			;;
		--manifest)
			shift
			manifest_path="${1:-}"
			;;
		--require-targets)
			shift
			require_targets="${1:-}"
			;;
		--require-bundles)
			require_bundles=1
			;;
		--verify-bundles-with-cosign)
			verify_bundles_with_cosign=1
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

if [[ -z "$manifest_path" ]]; then
	manifest_path="$bundle_dir/release-manifest.json"
fi

if [[ ! -f "$manifest_path" ]]; then
	echo "release manifest not found: $manifest_path" >&2
	exit 1
fi

if [[ -z "$checksums_path" ]]; then
	checksums_path="$bundle_dir/checksums-sha256.txt"
fi
checksums_basename="$(basename "$checksums_path")"

checksums_enabled=1
if [[ ! -f "$checksums_path" ]]; then
	if [[ $checksums_explicit -ne 0 ]]; then
		echo "checksums file not found: $checksums_path" >&2
		exit 1
	fi
	checksums_enabled=0
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

manifest_checksums_tmp="$(mktemp)"
cleanup_tmp() {
	rm -f "$manifest_checksums_tmp"
}
trap cleanup_tmp EXIT

python3 - "$manifest_path" "$bundle_dir" "$manifest_checksums_tmp" <<'PY'
import json
import re
import sys
from pathlib import Path

manifest_path = Path(sys.argv[1])
bundle_dir = Path(sys.argv[2])
output_path = Path(sys.argv[3])
obj = json.loads(manifest_path.read_text())

if obj.get("schema_version") != "codelens-release-manifest-v1":
    raise SystemExit(
        f"unexpected schema_version in {manifest_path}: {obj.get('schema_version')!r}"
    )
inventory_role = obj.get("inventory_role")
if inventory_role is not None and inventory_role != "authoritative":
    raise SystemExit(f"unexpected inventory_role in {manifest_path}: {inventory_role!r}")
inventory_scope = obj.get("inventory_scope")
if inventory_scope is not None and inventory_scope != "release_payloads":
    raise SystemExit(f"unexpected inventory_scope in {manifest_path}: {inventory_scope!r}")
checksums_role = obj.get("checksums_role")
if checksums_role is not None and checksums_role != "supplemental":
    raise SystemExit(f"unexpected checksums_role in {manifest_path}: {checksums_role!r}")
if not obj.get("repository"):
    raise SystemExit(f"missing repository in {manifest_path}")
if not obj.get("tag"):
    raise SystemExit(f"missing tag in {manifest_path}")
if not obj.get("version"):
    raise SystemExit(f"missing version in {manifest_path}")

assets = obj.get("assets")
if not isinstance(assets, list) or not assets:
    raise SystemExit(f"manifest assets must be a non-empty list in {manifest_path}")

payload_patterns = [
    re.compile(r"^codelens-mcp-airgap-.+\.tar\.gz$"),
    re.compile(r"^codelens-mcp-.+\.tar\.gz$"),
    re.compile(r"^codelens-mcp-.+\.zip$"),
    re.compile(r"^codelens-mcp-.+\.cdx\.json$"),
]

def is_payload_asset(name: str) -> bool:
    return any(pattern.match(name) for pattern in payload_patterns)

seen = set()
lines = []
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
    if not is_payload_asset(name):
        raise SystemExit(f"manifest contains unsupported asset name in {manifest_path}: {name!r}")
    if name in seen:
        raise SystemExit(f"duplicate manifest asset entry for {name} in {manifest_path}")
    seen.add(name)
    if not download_url.endswith("/" + name):
        raise SystemExit(f"manifest download_url does not end with asset name for {name} in {manifest_path}")
    if len(sha256) != 64 or not all(c in "0123456789abcdef" for c in sha256.lower()):
        raise SystemExit(f"invalid sha256 in {manifest_path} for {name}: {sha256!r}")
    asset_path = bundle_dir / name
    if not asset_path.is_file():
        raise SystemExit(f"manifest asset missing from bundle: {asset_path}")
    lines.append(f"{sha256}  {name}")

output_path.write_text("\n".join(lines) + "\n")
PY

manifest_assets=()
while IFS= read -r asset; do
	[[ -z "$asset" ]] && continue
	manifest_assets+=("$asset")
done < <(awk '{print $2}' "$manifest_checksums_tmp")

(
	cd "$bundle_dir"
	"$checksum_tool" "${checksum_args[@]}" "$manifest_checksums_tmp"
)

checksum_assets=()
if [[ $checksums_enabled -ne 0 ]]; then
	while IFS= read -r asset; do
		[[ -z "$asset" ]] && continue
		checksum_assets+=("$asset")
	done < <(awk '{print $2}' "$checksums_path")
	if [[ ${#checksum_assets[@]} -eq 0 ]]; then
		echo "checksums file is empty: $checksums_path" >&2
		exit 1
	fi

	duplicate_assets="$(printf '%s\n' "${checksum_assets[@]}" | LC_ALL=C sort | uniq -d)"
	if [[ -n "$duplicate_assets" ]]; then
		echo "checksums file contains duplicate artifact entries:" >&2
		printf '  %s\n' "$duplicate_assets" >&2
		exit 1
	fi

	for asset in "${checksum_assets[@]}"; do
		if [[ ! -f "$bundle_dir/$asset" ]]; then
			echo "missing artifact referenced by checksums: $bundle_dir/$asset" >&2
			exit 1
		fi
	done

	python3 - "$manifest_checksums_tmp" "$checksums_path" <<'PY'
import sys
from pathlib import Path

manifest_entries = {}
for raw_line in Path(sys.argv[1]).read_text().splitlines():
    line = raw_line.strip()
    if not line:
        continue
    checksum, name = line.split(maxsplit=1)
    manifest_entries[name] = checksum

checksum_entries = {}
for raw_line in Path(sys.argv[2]).read_text().splitlines():
    line = raw_line.strip()
    if not line:
        continue
    checksum, name = line.split(maxsplit=1)
    checksum_entries[name] = checksum

missing = sorted(name for name in manifest_entries if name not in checksum_entries)
mismatch = sorted(
    name for name, checksum in manifest_entries.items()
    if checksum_entries.get(name) not in (None, checksum)
)
if missing or mismatch:
    raise SystemExit(
        f"checksum manifest does not cover authoritative release manifest: missing={missing!r} mismatch={mismatch!r}"
    )
PY
fi

required_missing=0
required_targets=()
IFS=',' read -r -a required_targets <<< "$require_targets"
for target in "${required_targets[@]}"; do
	target="$(echo "$target" | xargs)"
	[[ -z "$target" ]] && continue
	if ! printf '%s\n' "${manifest_assets[@]}" | grep -Eq "^codelens-mcp-${target}\.(tar\.gz|zip)$"; then
		echo "missing expected target artifact: $target" >&2
		required_missing=1
	fi
done
if [[ $required_missing -ne 0 ]]; then
	exit 1
fi

if [[ $checksums_enabled -ne 0 ]]; then
	(
		cd "$(dirname "$checksums_path")"
		"$checksum_tool" "${checksum_args[@]}" "$(basename "$checksums_path")"
	)
fi

verify_tar_structure() {
	local archive="$1"
	local -a entries
	while IFS= read -r entry; do
		[[ -z "$entry" ]] && continue
		entries+=("$entry")
	done < <(tar -tzf "$archive")
	if [[ ${#entries[@]} -ne 1 ]]; then
		echo "unexpected tar contents in $archive: expected exactly 1 entry, got ${#entries[@]}" >&2
		return 1
	fi
	if [[ "${entries[0]}" != "codelens-mcp" ]]; then
		echo "unexpected tar payload in $archive: ${entries[0]}" >&2
		return 1
	fi
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
	if [[ ${#entries[@]} -ne 1 ]]; then
		echo "unexpected zip contents in $archive: expected exactly 1 entry, got ${#entries[@]}" >&2
		return 1
	fi
	if [[ "${entries[0]}" != "codelens-mcp.exe" ]]; then
		echo "unexpected zip payload in $archive: ${entries[0]}" >&2
		return 1
	fi
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

verify_sigstore_trusted_root() {
	local root_file="$1"
	python3 - "$root_file" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
lines = [line.strip() for line in path.read_text().splitlines() if line.strip()]
if not lines:
    raise SystemExit(f"trusted root file is empty: {path}")
for idx, line in enumerate(lines, start=1):
    obj = json.loads(line)
    if not isinstance(obj, dict) or not obj:
        raise SystemExit(f"trusted root line {idx} is not a non-empty JSON object in {path}")
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
	if command -v openssl >/dev/null 2>&1; then
		openssl x509 -in "$cert" -noout >/dev/null 2>&1 || {
			echo "invalid X.509 certificate file: $cert" >&2
			return 1
		}
	else
		grep -q "BEGIN CERTIFICATE" "$cert" || {
			echo "certificate file missing PEM header: $cert" >&2
			return 1
		}
	fi
}

verify_bundle_file() {
	local bundle="$1"
	local base="${bundle%.sigstore.json}"
	is_signable_asset "$(basename "$base")" || {
		echo "unexpected bundle sidecar without signable payload: $bundle" >&2
		return 1
	}
	if [[ ! -s "$bundle" ]]; then
		echo "bundle file is empty: $bundle" >&2
		return 1
	fi
	python3 - "$bundle" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
obj = json.loads(path.read_text())
if not isinstance(obj, dict):
    raise SystemExit(f"bundle is not a JSON object: {path}")
if not obj:
    raise SystemExit(f"bundle JSON is empty: {path}")
if not any(
    key in obj
    for key in (
        "verificationMaterial",
        "messageSignature",
        "dsseEnvelope",
        "Payload",
        "base64Signature",
        "Base64Signature",
    )
):
    raise SystemExit(f"bundle JSON missing expected verification fields: {path}")
PY
}

verify_bundle_with_cosign() {
	local artifact="$1"
	local bundle="$2"
	if [[ $verify_bundles_with_cosign -eq 0 ]]; then
		return 0
	fi
	if ! command -v cosign >/dev/null 2>&1; then
		echo "cosign is required for --verify-bundles-with-cosign" >&2
		return 1
	fi
	cosign verify-blob "$artifact" \
		--bundle "$bundle" \
		--certificate-identity-regexp "$certificate_identity_regexp" \
		--certificate-oidc-issuer "$certificate_oidc_issuer" >/dev/null 2>&1 || {
		echo "cosign bundle verification failed for $artifact using $bundle" >&2
		return 1
	}
}

is_signable_asset() {
	if [[ "$1" == "$checksums_basename" ]]; then
		return 0
	fi
	case "$1" in
		codelens-mcp-airgap-*.tar.gz|codelens-mcp-*.tar.gz|codelens-mcp-*.zip|codelens-mcp-*.cdx.json|release-manifest.json)
			return 0
			;;
		*)
			return 1
			;;
	esac
}

signable_assets=("${manifest_assets[@]}" "$(basename "$manifest_path")")
if [[ $checksums_enabled -ne 0 ]]; then
	signable_assets+=("$(basename "$checksums_path")")
fi

signed_release_surface=1

checksums_signature_path="${checksums_path}.sig"
checksums_certificate_path="${checksums_path}.pem"
checksums_bundle_path="${checksums_path}.sigstore.json"
if [[ $checksums_enabled -ne 0 && $signed_release_surface -ne 0 ]]; then
	if [[ ! -f "$checksums_signature_path" ]]; then
		echo "missing signature sidecar for checksum manifest: $checksums_signature_path" >&2
		exit 1
	fi
	if [[ ! -f "$checksums_certificate_path" ]]; then
		echo "missing certificate sidecar for checksum manifest: $checksums_certificate_path" >&2
		exit 1
	fi
	verify_signature_file "$checksums_signature_path"
	verify_certificate_file "$checksums_certificate_path"
fi

bundle_release_surface=$require_bundles
for asset in "${signable_assets[@]}"; do
	if [[ -f "$bundle_dir/$asset.sigstore.json" ]]; then
		bundle_release_surface=1
		break
	fi
done

if [[ $checksums_enabled -ne 0 && $bundle_release_surface -ne 0 ]]; then
	if [[ ! -f "$checksums_bundle_path" ]]; then
		echo "missing Sigstore bundle for checksum manifest: $checksums_bundle_path" >&2
		exit 1
	fi
	verify_bundle_file "$checksums_bundle_path"
	verify_bundle_with_cosign "$checksums_path" "$checksums_bundle_path"
fi

for asset in "${signable_assets[@]}"; do
	if [[ ! -f "$bundle_dir/$asset.sig" ]]; then
		echo "missing signature sidecar for $asset: $bundle_dir/$asset.sig" >&2
		exit 1
	fi
	if [[ ! -f "$bundle_dir/$asset.pem" ]]; then
		echo "missing certificate sidecar for $asset: $bundle_dir/$asset.pem" >&2
		exit 1
	fi
	verify_signature_file "$bundle_dir/$asset.sig"
	verify_certificate_file "$bundle_dir/$asset.pem"
	if [[ $bundle_release_surface -ne 0 ]]; then
		if [[ ! -f "$bundle_dir/$asset.sigstore.json" ]]; then
			echo "missing Sigstore bundle sidecar for $asset: $bundle_dir/$asset.sigstore.json" >&2
			exit 1
		fi
		verify_bundle_file "$bundle_dir/$asset.sigstore.json"
		verify_bundle_with_cosign "$bundle_dir/$asset" "$bundle_dir/$asset.sigstore.json"
	fi
done

validation_assets=("${manifest_assets[@]}" "$(basename "$manifest_path")")
if [[ -f "$bundle_dir/sigstore-trusted-root.jsonl" ]]; then
	validation_assets+=("sigstore-trusted-root.jsonl")
fi
if [[ $checksums_enabled -ne 0 ]]; then
	validation_assets+=("$(basename "$checksums_path")")
fi

for asset in "${validation_assets[@]}"; do
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
			# Already validated as the authoritative payload inventory before
			# checksum compatibility checks run.
			;;
		"$checksums_basename")
			# Supplemental manifest already verified above when present.
			;;
		sigstore-trusted-root.jsonl)
			verify_sigstore_trusted_root "$bundle_dir/$asset"
			;;
		*)
			echo "unexpected release artifact type in authoritative inventory: $asset" >&2
			exit 1
			;;
	esac
done

echo "verified release bundle:"
echo "  bundle_dir: $bundle_dir"
echo "  manifest:   $(basename "$manifest_path")"
if [[ $checksums_enabled -ne 0 ]]; then
	echo "  checksums:  $(basename "$checksums_path")"
else
	echo "  checksums:  not present"
fi
echo "  assets:     ${#validation_assets[@]}"
printf '  targets:    %s\n' "$require_targets"
if [[ $checksums_enabled -ne 0 ]]; then
	echo "  inventory:  authoritative manifest + supplemental checksums"
else
	echo "  inventory:  authoritative manifest only"
fi
if [[ $bundle_release_surface -ne 0 ]]; then
	echo "  bundles:    required"
fi
if [[ $verify_bundles_with_cosign -ne 0 ]]; then
	echo "  cosign:     bundle verification enabled"
fi
