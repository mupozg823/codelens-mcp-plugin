#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: scripts/verify-github-attestations.sh [bundle_dir] [options]

Verifies GitHub artifact attestations for packaged release archives. Each archive
is checked against both the default SLSA provenance predicate and the configured
SBOM predicate type.

Arguments:
  bundle_dir                    Directory containing release archives.
                                Defaults to current directory.

Options:
  --repo OWNER/REPO            Repository linked to the attestation subjects.
                               Default: mupozg823/codelens-mcp-plugin
  --signer-workflow PATH       Expected signer workflow identity for attestation
                               verification.
                               Default: mupozg823/codelens-mcp-plugin/.github/workflows/release.yml
  --require-targets LIST       Comma-separated expected release targets.
                               Default: darwin-arm64,linux-x86_64,windows-x86_64
  --download-bundles-dir DIR   Download attestation bundles to DIR before
                               verification and verify against those local bundles.
  --offline-bundles-dir DIR    Verify using pre-downloaded bundle JSONL files in
                               DIR instead of fetching via the GitHub API.
  --custom-trusted-root PATH   Pass a trusted_root.jsonl file to gh verification.
  --sbom-predicate-type URI    Predicate type used for SBOM attestations.
                               Default: https://cyclonedx.org/bom
  --skip-provenance            Skip provenance attestation verification.
  --skip-sbom                  Skip SBOM attestation verification.
  -h, --help                   Show this help.

Examples:
  scripts/verify-github-attestations.sh ./release-bundle
  scripts/verify-github-attestations.sh ./release-bundle --download-bundles-dir ./attestations
  scripts/verify-github-attestations.sh ./release-bundle --offline-bundles-dir ./attestations
EOF
}

bundle_dir="."
repo="${CODELENS_ATTEST_REPO:-mupozg823/codelens-mcp-plugin}"
signer_workflow="${CODELENS_ATTEST_SIGNER_WORKFLOW:-mupozg823/codelens-mcp-plugin/.github/workflows/release.yml}"
require_targets="darwin-arm64,linux-x86_64,windows-x86_64"
download_bundles_dir=""
offline_bundles_dir=""
custom_trusted_root=""
sbom_predicate_type="${CODELENS_SBOM_PREDICATE_TYPE:-https://cyclonedx.org/bom}"
skip_provenance=0
skip_sbom=0

while (($# > 0)); do
	case "$1" in
		-h|--help)
			usage
			exit 0
			;;
		--repo)
			shift
			repo="${1:-}"
			;;
		--signer-workflow)
			shift
			signer_workflow="${1:-}"
			;;
		--require-targets)
			shift
			require_targets="${1:-}"
			;;
		--download-bundles-dir)
			shift
			download_bundles_dir="${1:-}"
			;;
		--offline-bundles-dir)
			shift
			offline_bundles_dir="${1:-}"
			;;
		--custom-trusted-root)
			shift
			custom_trusted_root="${1:-}"
			;;
		--sbom-predicate-type)
			shift
			sbom_predicate_type="${1:-}"
			;;
		--skip-provenance)
			skip_provenance=1
			;;
		--skip-sbom)
			skip_sbom=1
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
bundle_dir="$(cd "$bundle_dir" && pwd)"

discovered_trusted_root="$bundle_dir/sigstore-trusted-root.jsonl"
if [[ -z "$custom_trusted_root" && -f "$discovered_trusted_root" ]]; then
	custom_trusted_root="$discovered_trusted_root"
fi

if [[ -n "$download_bundles_dir" && -n "$offline_bundles_dir" ]]; then
	echo "--download-bundles-dir and --offline-bundles-dir are mutually exclusive" >&2
	exit 2
fi

if [[ $skip_provenance -ne 0 && $skip_sbom -ne 0 ]]; then
	echo "nothing to verify: both provenance and sbom checks are disabled" >&2
	exit 2
fi

if ! command -v gh >/dev/null 2>&1; then
	echo "missing required tool: gh" >&2
	exit 1
fi

if [[ -n "$download_bundles_dir" ]]; then
	mkdir -p "$download_bundles_dir"
	download_bundles_dir="$(cd "$download_bundles_dir" && pwd)"
fi

if [[ -n "$offline_bundles_dir" && ! -d "$offline_bundles_dir" ]]; then
	echo "offline bundle directory does not exist: $offline_bundles_dir" >&2
	exit 1
fi
if [[ -n "$offline_bundles_dir" ]]; then
	offline_bundles_dir="$(cd "$offline_bundles_dir" && pwd)"
fi
if [[ -n "$custom_trusted_root" && ! -f "$custom_trusted_root" ]]; then
	echo "trusted root file does not exist: $custom_trusted_root" >&2
	exit 1
fi
if [[ -n "$custom_trusted_root" ]]; then
	custom_trusted_root="$(cd "$(dirname "$custom_trusted_root")" && pwd)/$(basename "$custom_trusted_root")"
fi

digest_for_file() {
	local path="$1"
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$path" | awk '{print $1}'
	elif command -v shasum >/dev/null 2>&1; then
		shasum -a 256 "$path" | awk '{print $1}'
	else
		echo "missing checksum tool: need sha256sum or shasum" >&2
		return 1
	fi
}

bundle_path_for_artifact() {
	local artifact="$1"
	local dir="$2"
	local digest bundle_path
	digest="$(digest_for_file "$artifact")"
	for bundle_path in \
		"$dir/sha256:${digest}.jsonl" \
		"$dir/sha256-${digest}.jsonl"; do
		if [[ -f "$bundle_path" ]]; then
			printf '%s\n' "$bundle_path"
			return 0
		fi
	done
	echo "missing attestation bundle for $(basename "$artifact") in $dir" >&2
	return 1
}

download_bundle_for_artifact() {
	local artifact="$1"
	local output_dir="$2"
	(
		cd "$output_dir"
		gh attestation download "$artifact" --repo "$repo" >/dev/null
	)
	bundle_path_for_artifact "$artifact" "$output_dir"
}

verify_attestation() {
	local artifact="$1"
	local mode="$2"
	local bundle_path="${3:-}"
	local -a args
	args=(attestation verify "$artifact" --repo "$repo" --signer-workflow "$signer_workflow")
	if [[ -n "$bundle_path" ]]; then
		args+=(--bundle "$bundle_path")
	fi
	if [[ -n "$custom_trusted_root" ]]; then
		args+=(--custom-trusted-root "$custom_trusted_root")
	fi
	case "$mode" in
		provenance)
			;;
		sbom)
			args+=(--predicate-type "$sbom_predicate_type")
			;;
		*)
			echo "unknown attestation mode: $mode" >&2
			return 1
			;;
	esac
	gh "${args[@]}" >/dev/null
}

archives=()
while IFS= read -r asset; do
	[[ -z "$asset" ]] && continue
	case "$(basename "$asset")" in
		codelens-mcp-airgap-*.tar.gz)
			continue
			;;
	esac
	archives+=("$asset")
done < <(
	find "$bundle_dir" -maxdepth 1 -type f \
		\( -name 'codelens-mcp-*.tar.gz' -o -name 'codelens-mcp-*.zip' \) \
		-print | LC_ALL=C sort
)

if [[ ${#archives[@]} -eq 0 ]]; then
	echo "no release archives found in $bundle_dir" >&2
	exit 1
fi

required_targets_arr=()
missing_targets=0
IFS=',' read -r -a required_targets_arr <<< "$require_targets"
for target in "${required_targets_arr[@]}"; do
	target="$(echo "$target" | xargs)"
	[[ -z "$target" ]] && continue
	if ! printf '%s\n' "${archives[@]}" | grep -Eq "/codelens-mcp-${target}\.(tar\.gz|zip)$"; then
		echo "missing expected target archive: $target" >&2
		missing_targets=1
	fi
done
if [[ $missing_targets -ne 0 ]]; then
	exit 1
fi

verified_count=0
for artifact in "${archives[@]}"; do
	bundle_path=""
	if [[ -n "$download_bundles_dir" ]]; then
		bundle_path="$(download_bundle_for_artifact "$artifact" "$download_bundles_dir")"
	elif [[ -n "$offline_bundles_dir" ]]; then
		bundle_path="$(bundle_path_for_artifact "$artifact" "$offline_bundles_dir")"
	fi

	if [[ $skip_provenance -eq 0 ]]; then
		verify_attestation "$artifact" provenance "$bundle_path"
	fi
	if [[ $skip_sbom -eq 0 ]]; then
		verify_attestation "$artifact" sbom "$bundle_path"
	fi
	verified_count=$((verified_count + 1))
done

echo "verified GitHub attestations:"
echo "  bundle_dir:       $bundle_dir"
echo "  repo:             $repo"
echo "  signer_workflow:  $signer_workflow"
echo "  archives:         $verified_count"
printf '  targets:          %s\n' "$require_targets"
if [[ -n "$download_bundles_dir" ]]; then
	echo "  bundles:          downloaded to $download_bundles_dir"
elif [[ -n "$offline_bundles_dir" ]]; then
	echo "  bundles:          verified from $offline_bundles_dir"
else
	echo "  bundles:          fetched via GitHub API during verification"
fi
if [[ -n "$custom_trusted_root" ]]; then
	echo "  trusted_root:     $custom_trusted_root"
fi
if [[ $skip_provenance -eq 0 ]]; then
	echo "  provenance:       verified"
fi
if [[ $skip_sbom -eq 0 ]]; then
	echo "  sbom predicate:   $sbom_predicate_type"
fi
