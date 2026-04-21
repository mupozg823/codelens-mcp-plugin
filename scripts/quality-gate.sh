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

configure_cargo_jobs() {
	if [[ -n "${CARGO_BUILD_JOBS:-}" ]]; then
		return
	fi
	if [[ -n "${CODELENS_CARGO_BUILD_JOBS:-}" ]]; then
		export CARGO_BUILD_JOBS="$CODELENS_CARGO_BUILD_JOBS"
		echo "[gate] using CODELENS_CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS"
		return
	fi
	if [[ "$(uname -s)" == "Darwin" ]]; then
		export CARGO_BUILD_JOBS=1
		echo "[gate] defaulting CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS on macOS to limit build memory pressure"
	fi
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
RUN_HARNESS_UNIT_GATE=0
RUN_AGENT_CONTRACT_GATE=0
RUN_NO_SEMANTIC_GATE=0
RUN_PHASE3_MATRIX_GATE=0
RUN_DATASET_LINT_GATE=0
PHASE3_REQUIRED_DATASETS="ripgrep,requests,jest,typescript,next-js,react-core,django,axum"

if [[ "$MODE" == "ci" ]]; then
	RUN_RUST_GATE=1
	RUN_PY_GATE=1
	RUN_HTTP_GATE=1
	RUN_CLIPPY_GATE=1
	RUN_RELEASE_GATE=1
	RUN_HARNESS_UNIT_GATE=1
	RUN_AGENT_CONTRACT_GATE=1
	RUN_NO_SEMANTIC_GATE=1
	RUN_PHASE3_MATRIX_GATE=1
	RUN_DATASET_LINT_GATE=1
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

	if matches_any "benchmarks/harness/*.py" "benchmarks/harness/**/*.py"; then
		RUN_HARNESS_UNIT_GATE=1
	fi

	if matches_any "agents/*" "agents/**/*" "scripts/agent-contract-check.py"; then
		RUN_AGENT_CONTRACT_GATE=1
	fi

	if matches_any "benchmarks/embedding-quality-matrix.py" \
		"benchmarks/embedding-quality-v1.*-phase3*.json" \
		"benchmarks/embedding-quality-dataset-*.json" \
		"benchmarks/README.md" \
		"docs/benchmarks.md"; then
		RUN_PHASE3_MATRIX_GATE=1
	fi

	if matches_any "benchmarks/lint-datasets.py" \
		"benchmarks/embedding-quality-dataset-self.json" \
		"benchmarks/role-retrieval-dataset.json"; then
		RUN_DATASET_LINT_GATE=1
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

	if matches_any "Cargo.toml" "Cargo.lock" \
		"crates/codelens-mcp/*" "crates/codelens-mcp/**" \
		"scripts/quality-gate.sh" ".github/workflows/*" \
		"EVAL_CONTRACT.md" "CLAUDE.md"; then
		RUN_NO_SEMANTIC_GATE=1
	fi

	if [[ "$RUN_RELEASE_GATE" -eq 1 ]]; then
		RUN_RUST_GATE=1
	fi

	if [[ "$RUN_NO_SEMANTIC_GATE" -eq 1 ]]; then
		RUN_RUST_GATE=1
	fi
fi

if [[ "$RUN_RUST_GATE" -eq 0 && "$RUN_PY_GATE" -eq 0 && "$RUN_HARNESS_UNIT_GATE" -eq 0 && "$RUN_AGENT_CONTRACT_GATE" -eq 0 && "$RUN_PHASE3_MATRIX_GATE" -eq 0 && "$RUN_NO_SEMANTIC_GATE" -eq 0 && "$RUN_DATASET_LINT_GATE" -eq 0 ]]; then
	echo "[gate] no relevant Rust, harness-Python, matrix, no-semantic, or dataset files changed; skipping repo-local gate."
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

if [[ "$RUN_DATASET_LINT_GATE" -eq 1 ]]; then
	if has_cmd python3; then
		echo "[gate] running dataset hygiene lint"
		python3 benchmarks/lint-datasets.py --project .
	else
		echo "[gate] python3 not installed; skipping dataset lint gate."
	fi
fi

if [[ "$RUN_PHASE3_MATRIX_GATE" -eq 1 ]]; then
	if has_cmd python3; then
		echo "[gate] validating external phase3 embedding matrix"
		python3 benchmarks/embedding-quality-matrix.py \
			--require-datasets "$PHASE3_REQUIRED_DATASETS" >/dev/null
	else
		echo "[gate] python3 not installed; skipping phase3 matrix gate."
	fi
fi

if [[ "$RUN_HARNESS_UNIT_GATE" -eq 1 ]]; then
	if has_cmd python3; then
		echo "[gate] running harness runner/unit gate"
		python3 -m unittest discover -s benchmarks/harness/tests -p 'test_*.py'
	else
		echo "[gate] python3 not installed; skipping harness runner/unit gate."
	fi
fi

if [[ "$RUN_AGENT_CONTRACT_GATE" -eq 1 ]]; then
	if has_cmd python3; then
		echo "[gate] running strict agent contract gate"
		python3 scripts/agent-contract-check.py --strict
	else
		echo "[gate] python3 not installed; skipping strict agent contract gate."
	fi
fi

if [[ "$RUN_RUST_GATE" -eq 1 ]]; then
	if ! has_cmd cargo; then
		echo "[gate] cargo not installed; cannot run Rust gate."
		exit 0
	fi
	configure_cargo_jobs
	if [[ "$MODE" == "ci" ]]; then
		echo "[gate] running CI Rust gate from EVAL_CONTRACT.md"
		cargo check
		cargo test -p codelens-engine
		cargo test -p codelens-mcp
	elif [[ "$MODE" == "build" ]]; then
		echo "[gate] running build workflow Rust gate from EVAL_CONTRACT.md"
		cargo test -p codelens-engine
		cargo test -p codelens-mcp -- --skip returns_lsp_diagnostics --skip returns_workspace_symbols --skip returns_rename_plan
	else
		echo "[gate] running local stop-hook Rust gate from EVAL_CONTRACT.md"
		cargo check
		cargo test -p codelens-engine
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

	if [[ "$RUN_NO_SEMANTIC_GATE" -eq 1 ]]; then
		echo "[gate] running no-semantic MCP gate"
		cargo test -p codelens-mcp --no-default-features
	fi

	# Feature matrix: verify opt-in features compile cleanly
	if [[ "$MODE" == "ci" ]]; then
		echo "[gate] running feature matrix build verification"
		cargo check -p codelens-mcp --features otel
		cargo check -p codelens-mcp --features scip-backend
		cargo check -p codelens-mcp --features "http,otel"
		cargo check -p codelens-mcp --features "http,scip-backend"
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
