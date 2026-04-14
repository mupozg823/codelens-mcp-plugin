#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: scripts/publish-crates-workspace.sh [options]

Publish the workspace crates to crates.io in dependency order:
  1. codelens-engine
  2. codelens-mcp
  3. codelens-tui

Safe defaults:
  - dry-run by default
  - explicit --execute required for real publish
  - exact workspace order is preserved even when selecting a subset
  - dry-run automatically falls back to `cargo check` for downstream workspace
    crates when Cargo cannot verify an unpublished internal dependency against
    crates.io yet

Options:
  --execute                  Run real `cargo publish` instead of `--dry-run`.
  --root PATH                Repository root. Default: current directory.
  --target-dir PATH          Cargo target dir for verification/publish.
                             Default: target/publish-workspace
  --package NAME             Publish only the selected package. Repeatable.
  --registry NAME            Cargo registry name. Default: crates.io.
  --allow-dirty              Pass `--allow-dirty` through to `cargo publish`.
  --skip-existing            Skip packages already published at the workspace
                             version on crates.io. Not supported with --registry.
  --publish-wait-seconds N   Sleep between real publishes. Default: 30.
  -h, --help                 Show this help.

Examples:
  scripts/publish-crates-workspace.sh --allow-dirty
  scripts/publish-crates-workspace.sh --execute
  scripts/publish-crates-workspace.sh --execute --skip-existing
  scripts/publish-crates-workspace.sh --package codelens-mcp --allow-dirty

Notes:
  - Real publish uses the normal Cargo registry auth flow, typically
    `CARGO_REGISTRY_TOKEN`.
  - Subset publish assumes any earlier workspace dependency crates at the same
    version have already been published.
EOF
}

die() {
	echo "$*" >&2
	exit 1
}

contains() {
	local needle="$1"
	shift
	local item
	for item in "$@"; do
		if [[ "$item" == "$needle" ]]; then
			return 0
		fi
	done
	return 1
}

read_workspace_version() {
	local cargo_toml="$1"
	python3 - "$cargo_toml" <<'PY'
import re
import sys
from pathlib import Path

text = Path(sys.argv[1]).read_text(encoding="utf-8")
in_workspace_package = False
for raw_line in text.splitlines():
    line = raw_line.strip()
    if line.startswith("["):
        in_workspace_package = line == "[workspace.package]"
        continue
    if in_workspace_package:
        match = re.match(r'version\s*=\s*"([^"]+)"', line)
        if match:
            print(match.group(1))
            raise SystemExit(0)
raise SystemExit("could not find [workspace.package] version")
PY
}

crate_exists_on_crates_io() {
	local package="$1"
	local version="$2"
	local status

	status="$(curl -sS -o /dev/null -w '%{http_code}' \
		"https://crates.io/api/v1/crates/${package}/${version}")" || return 2

	case "$status" in
		200) return 0 ;;
		404) return 1 ;;
		*)
			echo "unexpected crates.io response for ${package}@${version}: HTTP ${status}" >&2
			return 2
			;;
	esac
}

workspace_dependencies() {
	case "$1" in
		codelens-engine)
			;;
		codelens-mcp|codelens-tui)
			echo "codelens-engine"
			;;
		*)
			die "unknown workspace package: $1"
			;;
	esac
}

can_run_publish_dry_run() {
	local package="$1"
	local dep

	if [[ -n "$registry" ]]; then
		return 1
	fi

	for dep in $(workspace_dependencies "$package"); do
		if ! crate_exists_on_crates_io "$dep" "$workspace_version"; then
			return 1
		fi
	done

	return 0
}

root="."
mode="dry-run"
registry=""
allow_dirty=0
skip_existing=0
publish_wait_seconds=30
target_dir=""
requested_packages=()
ordered_packages=()
workspace_packages=(
	"codelens-engine"
	"codelens-mcp"
	"codelens-tui"
)

while (($# > 0)); do
	case "$1" in
		-h|--help)
			usage
			exit 0
			;;
		--execute)
			mode="execute"
			;;
		--root)
			shift
			root="${1:-}"
			;;
		--target-dir)
			shift
			target_dir="${1:-}"
			;;
		--package)
			shift
			requested_packages+=("${1:-}")
			;;
		--registry)
			shift
			registry="${1:-}"
			;;
		--allow-dirty)
			allow_dirty=1
			;;
		--skip-existing)
			skip_existing=1
			;;
		--publish-wait-seconds)
			shift
			publish_wait_seconds="${1:-}"
			;;
		--*)
			die "unknown option: $1"
			;;
		*)
			die "unexpected positional argument: $1"
			;;
	esac
	shift || true
done

[[ -n "$root" ]] || die "--root requires a path"
[[ -d "$root" ]] || die "repository root does not exist: $root"
[[ "$publish_wait_seconds" =~ ^[0-9]+$ ]] || die "--publish-wait-seconds must be an integer"

if [[ -n "$registry" && "$skip_existing" -ne 0 ]]; then
	die "--skip-existing is only supported for the default crates.io registry"
fi

command -v cargo >/dev/null 2>&1 || die "missing required command: cargo"
command -v python3 >/dev/null 2>&1 || die "missing required command: python3"
if [[ "$skip_existing" -ne 0 || "$mode" == "dry-run" ]]; then
	command -v curl >/dev/null 2>&1 || die "missing required command: curl"
fi

cd "$root"

[[ -f Cargo.toml ]] || die "workspace Cargo.toml not found in $PWD"
workspace_version="$(read_workspace_version Cargo.toml)"
if [[ -z "$target_dir" ]]; then
	target_dir="${CARGO_TARGET_DIR:-$PWD/target/publish-workspace}"
fi

if [[ ${#requested_packages[@]} -eq 0 ]]; then
	ordered_packages=("${workspace_packages[@]}")
else
	for package in "${requested_packages[@]}"; do
		contains "$package" "${workspace_packages[@]}" \
			|| die "unknown workspace package: $package"
	done

	for package in "${workspace_packages[@]}"; do
		if contains "$package" "${requested_packages[@]}"; then
			ordered_packages+=("$package")
		fi
	done
fi

[[ ${#ordered_packages[@]} -gt 0 ]] || die "no packages selected for publish"

if [[ "$mode" == "execute" && "$allow_dirty" -eq 0 ]] && command -v git >/dev/null 2>&1; then
	if [[ -n "$(git status --short 2>/dev/null || true)" ]]; then
		die "workspace is dirty; publish from a clean checkout or pass --allow-dirty"
	fi
fi

echo "workspace version: ${workspace_version}"
echo "mode: ${mode}"
if [[ -n "$registry" ]]; then
	echo "registry: ${registry}"
else
	echo "registry: crates.io"
fi
echo "target dir: ${target_dir}"
echo "publish order: ${ordered_packages[*]}"
if [[ "$skip_existing" -ne 0 ]]; then
	echo "skip-existing: enabled"
fi
if [[ "$mode" == "execute" ]]; then
	echo "inter-publish wait: ${publish_wait_seconds}s"
fi

remaining_runs=0
for package in "${ordered_packages[@]}"; do
	if [[ "$skip_existing" -ne 0 ]] && crate_exists_on_crates_io "$package" "$workspace_version"; then
		echo "[skip] ${package}@${workspace_version} already exists on crates.io"
		continue
	fi
	remaining_runs=$((remaining_runs + 1))
done

if [[ "$remaining_runs" -eq 0 ]]; then
	echo "no publish actions required"
	exit 0
fi

run_index=0
for package in "${ordered_packages[@]}"; do
	if [[ "$skip_existing" -ne 0 ]] && crate_exists_on_crates_io "$package" "$workspace_version"; then
		continue
	fi

	run_index=$((run_index + 1))
	echo
	echo "[${run_index}/${remaining_runs}] ${package}@${workspace_version}"

	if [[ "$mode" == "dry-run" ]]; then
		if can_run_publish_dry_run "$package"; then
			cmd=(cargo publish --locked -p "$package" --dry-run)
			if [[ "$allow_dirty" -ne 0 ]]; then
				cmd+=(--allow-dirty)
			fi
		else
			deps="$(workspace_dependencies "$package")"
			if [[ -n "$registry" ]]; then
				echo "full publish dry-run unavailable for custom registry '${registry}' in this script"
			elif [[ -n "$deps" ]]; then
				echo "full publish dry-run unavailable until internal dependency is on crates.io: ${deps}@${workspace_version}"
			else
				echo "full publish dry-run unavailable for registry '${registry}'"
			fi
			echo "falling back to cargo check for compile verification"
			cmd=(cargo check --locked -p "$package")
		fi
	else
		cmd=(cargo publish --locked -p "$package")
		if [[ -n "$registry" ]]; then
			cmd+=(--registry "$registry")
		fi
		if [[ "$allow_dirty" -ne 0 ]]; then
			cmd+=(--allow-dirty)
		fi
	fi

	printf '+'
	printf ' %q' env CARGO_TARGET_DIR="$target_dir"
	printf ' %q' "${cmd[@]}"
	printf '\n'
	env CARGO_TARGET_DIR="$target_dir" "${cmd[@]}"

	if [[ "$mode" == "execute" && "$run_index" -lt "$remaining_runs" && "$publish_wait_seconds" -gt 0 ]]; then
		echo "waiting ${publish_wait_seconds}s for registry propagation"
		sleep "$publish_wait_seconds"
	fi
done
