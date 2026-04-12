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
	if [[ ${#entries[@]} -ne 1 ]]; then
		echo "unexpected tar contents in $archive: expected exactly 1 entry, got ${#entries[@]}" >&2
		return 1
	fi
	if [[ "${entries[0]}" != "codelens-mcp" ]]; then
		echo "unexpected tar payload in $archive: ${entries[0]}" >&2
		return 1
	fi
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

for asset in "${assets[@]}"; do
	case "$asset" in
		*.tar.gz)
			verify_tar_structure "$bundle_dir/$asset"
			;;
		*.zip)
			verify_zip_structure "$bundle_dir/$asset"
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
