#!/usr/bin/env bash
set -euo pipefail

MODE="local"

while [[ $# -gt 0 ]]; do
	case "$1" in
	--mode)
		MODE="${2:-}"
		shift 2
		;;
	*)
		echo "[gate] unknown argument: $1" >&2
		exit 2
		;;
	esac
done

if [[ "$MODE" != "local" && "$MODE" != "ci" && "$MODE" != "build" ]]; then
	echo "[gate] invalid mode: $MODE" >&2
	exit 2
fi

ROOT="$(pwd)"
if command -v git >/dev/null 2>&1 && git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
	ROOT="$(git rev-parse --show-toplevel)"
fi
cd "$ROOT"

has_cmd() {
	command -v "$1" >/dev/null 2>&1
}

changed_files() {
	if ! has_cmd git || ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
		return 0
	fi
	{
		git diff --name-only --cached || true
		git diff --name-only || true
		git ls-files --others --exclude-standard || true
	} | awk 'NF' | sort -u
}

matches_any() {
	local patterns=("$@")
	local files
	files="$(changed_files || true)"
	if [[ -z "$files" ]]; then
		return 0
	fi
	while IFS= read -r f; do
		for p in "${patterns[@]}"; do
			if [[ "$f" == $p ]]; then
				return 0
			fi
		done
	done <<<"$files"
	return 1
}

echo "[gate] root: $ROOT"

if [[ ! -f "$ROOT/EVAL_CONTRACT.md" ]]; then
	echo "[gate] no repo-local EVAL_CONTRACT.md; nothing to do."
	exit 0
fi

if ! has_cmd cargo; then
	echo "[gate] cargo not installed; Rust gate may be skipped."
fi

RUN_RUST_GATE=0
RUN_PY_GATE=0
RUN_HTTP_GATE=0
RUN_CLIPPY_GATE=0
RUN_RELEASE_GATE=0

if [[ "$MODE" == "ci" ]]; then
	RUN_RUST_GATE=1
	RUN_PY_GATE=1
	RUN_CLIPPY_GATE=1
	RUN_RELEASE_GATE=1
elif [[ "$MODE" == "build" ]]; then
	RUN_RUST_GATE=1
	RUN_RELEASE_GATE=1
else
	if matches_any "*.rs" "Cargo.toml" "Cargo.lock" "crates/*"; then
		RUN_RUST_GATE=1
	fi

	if matches_any "benchmarks/*.py" "benchmarks/**/*.py" "scripts/*.py"; then
		RUN_PY_GATE=1
	fi

	if matches_any "crates/codelens-mcp/src/server/*" "crates/codelens-mcp/src/transport*" "crates/codelens-mcp/src/resources*" "crates/codelens-mcp/src/dispatch*" "crates/codelens-mcp/src/session*" "crates/codelens-mcp/src/*http*" "crates/codelens-mcp/src/*resource*" "crates/codelens-mcp/src/state.rs"; then
		RUN_HTTP_GATE=1
	fi

	if [[ "$RUN_RUST_GATE" -eq 1 ]] && cargo clippy -V >/dev/null 2>&1; then
		RUN_CLIPPY_GATE=1
	fi

	if matches_any "Cargo.toml" "Cargo.lock" ".github/workflows/*" "Formula/*"; then
		RUN_RELEASE_GATE=1
	fi

	if [[ "$RUN_RELEASE_GATE" -eq 1 ]]; then
		RUN_RUST_GATE=1
	fi
fi

if [[ "$RUN_RUST_GATE" -eq 0 && "$RUN_PY_GATE" -eq 0 ]]; then
	echo "[gate] no Rust or harness-Python files changed; skipping repo-local gate."
	exit 0
fi

if [[ "$RUN_PY_GATE" -eq 1 ]]; then
	if has_cmd python3; then
		echo "[gate] running harness Python syntax gate"
		while IFS= read -r f; do
			[[ "$f" == *.py ]] || continue
			python3 -m py_compile "$f"
		done < <(changed_files || true)
	else
		echo "[gate] python3 not installed; skipping harness Python gate."
	fi
fi

if [[ "$RUN_RUST_GATE" -eq 1 ]]; then
	if ! has_cmd cargo; then
		echo "[gate] cargo not installed; cannot run Rust gate."
		exit 0
	fi
	if [[ "$MODE" == "ci" ]]; then
		echo "[gate] running CI Rust gate from EVAL_CONTRACT.md"
		cargo check
		cargo test -p codelens-core
		cargo test -p codelens-mcp
	elif [[ "$MODE" == "build" ]]; then
		echo "[gate] running build workflow Rust gate from EVAL_CONTRACT.md"
		cargo test -p codelens-core
		cargo test -p codelens-mcp -- --skip returns_lsp_diagnostics --skip returns_workspace_symbols --skip returns_rename_plan
	else
		echo "[gate] running local stop-hook Rust gate from EVAL_CONTRACT.md"
		cargo check
		cargo test -p codelens-core
		cargo test -p codelens-mcp
	fi

	if [[ "$MODE" == "local" && "$RUN_HTTP_GATE" -eq 1 ]]; then
		echo "[gate] running extended HTTP gate"
		cargo test -p codelens-mcp --features http
	fi

	if [[ "$RUN_CLIPPY_GATE" -eq 1 ]]; then
		echo "[gate] running clippy gate"
		cargo clippy -- -W clippy::all
	fi

	if [[ "$RUN_RELEASE_GATE" -eq 1 ]]; then
		echo "[gate] running release builds"
		if [[ "$MODE" == "build" ]]; then
			cargo build --release
		else
			cargo build --release --no-default-features
			cargo build --release
		fi
	fi
fi
