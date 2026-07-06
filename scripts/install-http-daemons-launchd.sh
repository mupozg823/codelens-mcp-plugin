#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: bash scripts/install-http-daemons-launchd.sh [repo-root] [options]

Build and install repo-local launchd agents for the recommended dual-daemon
CodeLens setup on macOS:
  - read-only reviewer/planner daemon
  - mutation-enabled builder/refactor daemon

Also writes repo-local host attach overrides into `.codelens/config.json`
so `codelens-mcp attach/status/doctor` reuse the same host -> URL contract.

Defaults in this repository follow the repo-local operating contract:
  - readonly: reviewer-graph on :7839
  - mutation: refactor-full on :7838

Options:
  --label-prefix PREFIX       launchd label prefix (default: dev.codelens.mcp)
  --bin-path PATH             stable http-capable binary path (default: <repo>/.codelens/bin/codelens-mcp-http)
  --launch-agents-dir DIR     directory for generated plist files (default: ~/Library/LaunchAgents)
  --readonly-port N           read-only daemon port (default: 7839)
  --mutation-port N           mutation daemon port (default: 7838)
  --readonly-profile NAME     read-only profile (default: reviewer-graph)
  --mutation-profile NAME     mutation profile (default: refactor-full)
  --readonly-log-level LEVEL  CODELENS_LOG for read-only daemon (default: warn)
  --mutation-log-level LEVEL  CODELENS_LOG for mutation daemon (default: warn)
  --effort-level LEVEL        CODELENS_EFFORT_LEVEL for both daemons (default: high)
  --response-contract MODE    CODELENS_RESPONSE_CONTRACT for both daemons
                              (default: full; lean = scaffold-only token-frugal
                              envelopes for token-expensive models, per-call
                              override via _lean argument)
  --lsp-prewarm MODE          CODELENS_LSP_PREWARM for both daemons
                              (default: off; auto = pre-warm servers for the
                              project's dominant indexed languages, or a
                              comma-separated server list)
  --rerank VALUE              CODELENS_RERANK for both daemons (default: 0)
  --embed-resource-profile P  CODELENS_EMBED_RESOURCE_PROFILE for semantic runtime
                              (default: low_power; use balanced/throughput to trade power for speed)
  --model-dir DIR             CODELENS_MODEL_DIR for semantic search model assets
                              (default: <repo>/crates/codelens-engine/models when present)
  --semantic                  build with http,semantic features (default)
  --no-semantic               build with http only and omit CODELENS_MODEL_DIR from plists
  --run-at-load               add RunAtLoad=true to generated plists
  --load                      bootstrap generated plists after writing
  --no-build                  reuse an existing http-capable binary at --bin-path
  --print-only                print both plists to stdout instead of writing them
                              (with --principals-scaffold also previews the scaffold)
  --principals-scaffold       write a commented RBAC starter to <repo>/.codelens/principals.toml
                              when absent; no-op if the file already exists. The starter's
                              active [default] role is ReadOnly — mutation tools stay denied
                              until [principal."<id>"] entries grant Refactor/Admin.
  -h, --help                  show help

Examples:
  bash scripts/install-http-daemons-launchd.sh .
  bash scripts/install-http-daemons-launchd.sh . --load
  bash scripts/install-http-daemons-launchd.sh . --readonly-port 7837 --mutation-port 7838
EOF
}

SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT=""
LABEL_PREFIX="dev.codelens.mcp"
BIN_PATH=""
LAUNCH_AGENTS_DIR="$HOME/Library/LaunchAgents"
READONLY_PORT=7839
MUTATION_PORT=7838
READONLY_PROFILE="reviewer-graph"
MUTATION_PROFILE="refactor-full"
READONLY_LOG_LEVEL="warn"
MUTATION_LOG_LEVEL="warn"
EFFORT_LEVEL="high"
RESPONSE_CONTRACT="full"
LSP_PREWARM="off"
RERANK_VALUE="0"
EMBED_RESOURCE_PROFILE="low_power"
MODEL_DIR=""
SEMANTIC=1
RUN_AT_LOAD=1
LOAD_AFTER_WRITE=0
NO_BUILD=0
PRINT_ONLY=0
PRINCIPALS_SCAFFOLD=0

is_int_in_range() {
	local value="$1"
	local min="$2"
	local max="$3"
	[[ "$value" =~ ^[0-9]+$ ]] || return 1
	((value >= min && value <= max))
}

xml_escape() {
	python3 -c '
import html
import sys

print(html.escape(sys.argv[1], quote=True))
' "$1"
}

while [[ $# -gt 0 ]]; do
	case "$1" in
	-h | --help)
		usage
		exit 0
		;;
	--label-prefix)
		LABEL_PREFIX="${2:-}"
		shift 2
		;;
	--bin-path)
		BIN_PATH="${2:-}"
		shift 2
		;;
	--launch-agents-dir)
		LAUNCH_AGENTS_DIR="${2:-}"
		shift 2
		;;
	--readonly-port)
		READONLY_PORT="${2:-}"
		shift 2
		;;
	--mutation-port)
		MUTATION_PORT="${2:-}"
		shift 2
		;;
	--readonly-profile)
		READONLY_PROFILE="${2:-}"
		shift 2
		;;
	--mutation-profile)
		MUTATION_PROFILE="${2:-}"
		shift 2
		;;
	--readonly-log-level)
		READONLY_LOG_LEVEL="${2:-}"
		shift 2
		;;
	--mutation-log-level)
		MUTATION_LOG_LEVEL="${2:-}"
		shift 2
		;;
	--effort-level)
		EFFORT_LEVEL="${2:-}"
		shift 2
		;;
	--response-contract)
		RESPONSE_CONTRACT="${2:-}"
		shift 2
		;;
	--lsp-prewarm)
		LSP_PREWARM="${2:-}"
		shift 2
		;;
	--rerank)
		RERANK_VALUE="${2:-}"
		shift 2
		;;
	--embed-resource-profile)
		EMBED_RESOURCE_PROFILE="${2:-}"
		shift 2
		;;
	--model-dir)
		MODEL_DIR="${2:-}"
		shift 2
		;;
	--semantic)
		SEMANTIC=1
		shift
		;;
	--no-semantic)
		SEMANTIC=0
		shift
		;;
	--run-at-load)
		RUN_AT_LOAD=1
		shift
		;;
	--load)
		LOAD_AFTER_WRITE=1
		shift
		;;
	--no-build)
		NO_BUILD=1
		shift
		;;
	--print-only)
		PRINT_ONLY=1
		shift
		;;
	--principals-scaffold)
		PRINCIPALS_SCAFFOLD=1
		shift
		;;
	-*)
		echo "unknown option: $1" >&2
		usage >&2
		exit 2
		;;
	*)
		if [[ -n "$REPO_ROOT" ]]; then
			echo "multiple repo roots provided" >&2
			usage >&2
			exit 2
		fi
		REPO_ROOT="$1"
		shift
		;;
	esac
done

if [[ -z "$REPO_ROOT" ]]; then
	REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"
else
	REPO_ROOT="$(cd -- "$REPO_ROOT" && pwd)"
fi

if [[ ! -f "$REPO_ROOT/Cargo.toml" || ! -f "$REPO_ROOT/crates/codelens-mcp/Cargo.toml" ]]; then
	echo "repo root does not look like codelens-mcp-plugin: $REPO_ROOT" >&2
	exit 1
fi

if ! is_int_in_range "$READONLY_PORT" 1 65535; then
	echo "--readonly-port must be an integer in [1, 65535]" >&2
	exit 2
fi
if ! is_int_in_range "$MUTATION_PORT" 1 65535; then
	echo "--mutation-port must be an integer in [1, 65535]" >&2
	exit 2
fi

if [[ -z "$BIN_PATH" ]]; then
	BIN_PATH="$REPO_ROOT/.codelens/bin/codelens-mcp-http"
fi

if [[ "$SEMANTIC" == "1" && -z "$MODEL_DIR" ]]; then
	default_model_dir="$REPO_ROOT/crates/codelens-engine/models"
	if [[ -d "$default_model_dir" ]]; then
		MODEL_DIR="$default_model_dir"
	fi
fi

if [[ -n "$MODEL_DIR" ]]; then
	if [[ ! -d "$MODEL_DIR" ]]; then
		echo "--model-dir does not exist or is not a directory: $MODEL_DIR" >&2
		exit 2
	fi
	MODEL_DIR="$(cd -- "$MODEL_DIR" && pwd)"
fi

mkdir -p "$(dirname "$BIN_PATH")"
LOG_DIR="$REPO_ROOT/.codelens/reports/launchd"
mkdir -p "$LOG_DIR"

if [[ "$NO_BUILD" == "0" ]]; then
	BUILD_FEATURES="http"
	if [[ "$SEMANTIC" == "1" ]]; then
		BUILD_FEATURES="http,semantic"
	fi
	echo "==> Building codelens-mcp with features=$BUILD_FEATURES from $REPO_ROOT"
	# Issue #227: cargo caches build script output across incremental
	# rebuilds, so source-only changes leave CODELENS_BUILD_TIME
	# pointing at the previous build. Touch the build script before
	# every release/install build so cargo re-runs it and embeds the
	# current timestamp — daemon freshness verification then has a
	# trustworthy `built` value in `--version` and capabilities output.
	BUILD_RS="$REPO_ROOT/crates/codelens-mcp/build.rs"
	if [[ -f "$BUILD_RS" ]]; then
		touch "$BUILD_RS"
	fi
	(
		cd "$REPO_ROOT"
		cargo build -p codelens-mcp --release --features "$BUILD_FEATURES"
	)
	SOURCE_BIN="$REPO_ROOT/target/release/codelens-mcp"
	if [[ ! -x "$SOURCE_BIN" ]]; then
		echo "expected built binary not found: $SOURCE_BIN" >&2
		exit 1
	fi
	install -m 755 "$SOURCE_BIN" "$BIN_PATH"
	echo "==> Installed http binary to $BIN_PATH"
elif [[ ! -x "$BIN_PATH" ]]; then
	echo "--no-build was set but binary is missing or not executable: $BIN_PATH" >&2
	exit 1
fi

# Issue #238: macOS launchd silently kills freshly-rebuilt binaries with
# `OS_REASON_CODESIGNING` (Hardened Runtime / Gatekeeper enforcement),
# leaving the daemons in `state = spawn scheduled` with no live process
# and no signal at the CodeLens MCP layer. Ad-hoc sign here so every
# redeploy from the dogfood loop comes back up cleanly. Best-effort:
# `codesign` failure is logged but not fatal so non-Apple-tooled hosts
# still complete the install.
#
# Issue #286: even after a clean ad-hoc sign, a `cp` from another volume
# or a downloaded artifact can carry `com.apple.quarantine` xattrs that
# trigger the same Gatekeeper rejection. Strip xattrs before signing so
# this step is idempotent regardless of how the binary landed on disk.
if [[ "$(uname)" == "Darwin" ]]; then
	if command -v xattr >/dev/null 2>&1; then
		xattr -cr "$BIN_PATH" 2>/dev/null || true
	fi
	if command -v codesign >/dev/null 2>&1; then
		echo "==> Ad-hoc signing http binary (macOS Hardened Runtime)"
		codesign -s - --force \
			--preserve-metadata=identifier,entitlements,flags,runtime \
			"$BIN_PATH" || {
			echo "warning: codesign failed; daemons may be killed by Gatekeeper" >&2
		}
		# Verify the signature actually applied — a silent codesign failure
		# (rare, but seen on partial Xcode installs) would still let the
		# binary through to launchd where it dies with OS_REASON_CODESIGNING.
		if ! codesign --verify --strict "$BIN_PATH" 2>/dev/null; then
			echo "warning: codesign --verify failed for $BIN_PATH; daemons will likely fail to launch" >&2
		fi
	fi
fi

readonly_label="${LABEL_PREFIX}-readonly"
mutation_label="${LABEL_PREFIX}-mutation"
readonly_plist="$LAUNCH_AGENTS_DIR/${readonly_label}.plist"
mutation_plist="$LAUNCH_AGENTS_DIR/${mutation_label}.plist"

create_plist() {
	local label="$1"
	local profile="$2"
	local daemon_mode="$3"
	local port="$4"
	local log_level="$5"
	local stdout_path="$6"
	local stderr_path="$7"
	local plist_path="$8"

	local run_at_load_xml=""
	if [[ "$RUN_AT_LOAD" == "1" ]]; then
		run_at_load_xml=$'  <key>RunAtLoad</key>\n  <true/>\n'
	fi

	local label_xml
	local bin_xml
	local repo_xml
	local stdout_xml
	local stderr_xml
	local embed_resource_profile_xml
	local model_dir_xml=""
	label_xml="$(xml_escape "$label")"
	bin_xml="$(xml_escape "$BIN_PATH")"
	repo_xml="$(xml_escape "$REPO_ROOT")"
	stdout_xml="$(xml_escape "$stdout_path")"
	stderr_xml="$(xml_escape "$stderr_path")"
	embed_resource_profile_xml="$(xml_escape "$EMBED_RESOURCE_PROFILE")"
	if [[ "$SEMANTIC" == "1" && -n "$MODEL_DIR" ]]; then
		model_dir_xml=$'    <key>CODELENS_MODEL_DIR</key>\n    <string>'"$(xml_escape "$MODEL_DIR")"$'</string>\n'
	fi

	mkdir -p "$(dirname "$plist_path")"
	{
		printf '%s\n' '<?xml version="1.0" encoding="UTF-8"?>'
		printf '%s\n' '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">'
		printf '%s\n' '<plist version="1.0">'
		printf '%s\n' '<dict>'
		printf '%s\n' '  <key>Label</key>'
		printf '  <string>%s</string>\n' "$label_xml"
		printf '%s\n' '  <key>ProgramArguments</key>'
		printf '%s\n' '  <array>'
		printf '    <string>%s</string>\n' "$bin_xml"
		printf '    <string>%s</string>\n' "$repo_xml"
		printf '%s\n' '    <string>--transport</string>'
		printf '%s\n' '    <string>http</string>'
		printf '%s\n' '    <string>--profile</string>'
		printf '    <string>%s</string>\n' "$profile"
		printf '%s\n' '    <string>--daemon-mode</string>'
		printf '    <string>%s</string>\n' "$daemon_mode"
		printf '%s\n' '    <string>--port</string>'
		printf '    <string>%s</string>\n' "$port"
		printf '%s\n' '  </array>'
		printf '%s\n' '  <key>WorkingDirectory</key>'
		printf '  <string>%s</string>\n' "$repo_xml"
		printf '%s\n' '  <key>EnvironmentVariables</key>'
		printf '%s\n' '  <dict>'
		printf '%s\n' '    <key>CODELENS_LOG</key>'
		printf '    <string>%s</string>\n' "$log_level"
		printf '%s\n' '    <key>CODELENS_EFFORT_LEVEL</key>'
		printf '    <string>%s</string>\n' "$EFFORT_LEVEL"
		printf '%s\n' '    <key>CODELENS_RESPONSE_CONTRACT</key>'
		printf '    <string>%s</string>\n' "$RESPONSE_CONTRACT"
		printf '%s\n' '    <key>CODELENS_LSP_PREWARM</key>'
		printf '    <string>%s</string>\n' "$LSP_PREWARM"
		printf '%s\n' '    <key>CODELENS_RERANK</key>'
		printf '    <string>%s</string>\n' "$RERANK_VALUE"
		printf '%s\n' '    <key>CODELENS_EMBED_RESOURCE_PROFILE</key>'
		printf '    <string>%s</string>\n' "$embed_resource_profile_xml"
		if [[ -n "$model_dir_xml" ]]; then
			printf '%s' "$model_dir_xml"
		fi
		printf '%s\n' '  </dict>'
		printf '%s\n' '  <key>KeepAlive</key>'
		printf '%s\n' '  <dict>'
		printf '%s\n' '    <key>SuccessfulExit</key>'
		printf '%s\n' '    <true/>'
		printf '%s\n' '    <key>Crashed</key>'
		printf '%s\n' '    <true/>'
		printf '%s\n' '  </dict>'
		if [[ -n "$run_at_load_xml" ]]; then
			printf '%s' "$run_at_load_xml"
		fi
		printf '%s\n' '  <key>StandardOutPath</key>'
		printf '  <string>%s</string>\n' "$stdout_xml"
		printf '%s\n' '  <key>StandardErrorPath</key>'
		printf '  <string>%s</string>\n' "$stderr_xml"
		printf '%s\n' '</dict>'
		printf '%s\n' '</plist>'
	} >"$plist_path"

	if command -v plutil >/dev/null 2>&1; then
		plutil -lint "$plist_path" >/dev/null
	fi
}

update_host_attach_config() {
	local config_path="$REPO_ROOT/.codelens/config.json"
	local readonly_url="http://127.0.0.1:${READONLY_PORT}/mcp"
	local mutation_url="http://127.0.0.1:${MUTATION_PORT}/mcp"
	local update_config_py

	mkdir -p "$(dirname "$config_path")"
	update_config_py='
import json
import pathlib
import sys

config_path = pathlib.Path(sys.argv[1])
readonly_url = sys.argv[2]
mutation_url = sys.argv[3]

payload = {}
if config_path.exists():
    payload = json.loads(config_path.read_text())
    if not isinstance(payload, dict):
        raise SystemExit(f"{config_path} must contain a JSON object")

host_attach = payload.setdefault("host_attach", {})
if not isinstance(host_attach, dict):
    raise SystemExit("host_attach must be a JSON object")

per_host_urls = host_attach.setdefault("per_host_urls", {})
if not isinstance(per_host_urls, dict):
    raise SystemExit("host_attach.per_host_urls must be a JSON object")

per_host_urls["claude-code"] = readonly_url
per_host_urls["cursor"] = readonly_url
per_host_urls["codex"] = mutation_url

config_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
'
	python3 -c "$update_config_py" "$config_path" "$readonly_url" "$mutation_url"
	echo "==> Updated host attach overrides in $config_path"
}

# Starter content for <repo>/.codelens/principals.toml. Keys must stay
# exactly what crates/codelens-mcp/src/principals.rs parses:
# [default].role and [principal."<id>"].role, with the role strings
# "ReadOnly" | "Refactor" | "Admin".
principals_scaffold_content() {
	cat <<'EOF'
# CodeLens RBAC principals (parsed by crates/codelens-mcp/src/principals.rs).
# Discovery order: <project>/.codelens/principals.toml, then
# ~/.codelens/principals.toml (first existing file wins).
# Roles: "ReadOnly" | "Refactor" | "Admin"; hierarchy Admin > Refactor > ReadOnly.
# Principal ids come from the HTTP JWT `sub` claim (or the
# X-Codelens-Principal header in dev mode); stdio falls back to the
# CODELENS_PRINCIPAL env var.
#
# Without this file a mutation-capable daemon maps every principal to
# Refactor (permissive default). This starter denies mutations by
# default: keep [default] at ReadOnly and grant Refactor/Admin per id.

# Role for any principal id not listed below, and for requests that
# carry no principal id at all.
[default]
role = "ReadOnly"

# Planner/reviewer sessions: navigation and analysis only.
# [principal."planner"]
# role = "ReadOnly"

# Builder sessions: code-mutation tools allowed.
# [principal."builder"]
# role = "Refactor"

# Operator access, including audit_log_query.
# [principal."ops@example.com"]
# role = "Admin"
EOF
}

scaffold_principals_toml() {
	local principals_path="$REPO_ROOT/.codelens/principals.toml"
	if [[ -e "$principals_path" ]]; then
		echo "==> principals.toml already exists, leaving untouched: $principals_path"
		return 0
	fi
	mkdir -p "$(dirname "$principals_path")"
	principals_scaffold_content >"$principals_path"
	echo "==> Wrote RBAC principals scaffold to $principals_path"
	echo "==> Scaffold default role is ReadOnly; add [principal.\"<id>\"] entries to grant Refactor/Admin"
}

readonly_stdout="$LOG_DIR/${readonly_label}.out.log"
readonly_stderr="$LOG_DIR/${readonly_label}.err.log"
mutation_stdout="$LOG_DIR/${mutation_label}.out.log"
mutation_stderr="$LOG_DIR/${mutation_label}.err.log"

if [[ "$PRINT_ONLY" == "1" ]]; then
	tmpdir="$(mktemp -d)"
	trap 'rm -rf "$tmpdir"' EXIT
	create_plist "$readonly_label" "$READONLY_PROFILE" "read-only" "$READONLY_PORT" "$READONLY_LOG_LEVEL" "$readonly_stdout" "$readonly_stderr" "$tmpdir/readonly.plist"
	create_plist "$mutation_label" "$MUTATION_PROFILE" "mutation-enabled" "$MUTATION_PORT" "$MUTATION_LOG_LEVEL" "$mutation_stdout" "$mutation_stderr" "$tmpdir/mutation.plist"
	echo "== ${tmpdir}/readonly.plist =="
	cat "$tmpdir/readonly.plist"
	echo
	echo "== ${tmpdir}/mutation.plist =="
	cat "$tmpdir/mutation.plist"
	if [[ "$PRINCIPALS_SCAFFOLD" == "1" ]]; then
		echo
		echo "== ${REPO_ROOT}/.codelens/principals.toml (scaffold preview) =="
		if [[ -e "$REPO_ROOT/.codelens/principals.toml" ]]; then
			echo "# principals.toml already exists; scaffold would be a no-op"
		else
			principals_scaffold_content
		fi
	fi
	exit 0
fi

create_plist "$readonly_label" "$READONLY_PROFILE" "read-only" "$READONLY_PORT" "$READONLY_LOG_LEVEL" "$readonly_stdout" "$readonly_stderr" "$readonly_plist"
create_plist "$mutation_label" "$MUTATION_PROFILE" "mutation-enabled" "$MUTATION_PORT" "$MUTATION_LOG_LEVEL" "$mutation_stdout" "$mutation_stderr" "$mutation_plist"
update_host_attach_config
if [[ "$PRINCIPALS_SCAFFOLD" == "1" ]]; then
	scaffold_principals_toml
fi

echo "==> Wrote $readonly_plist"
echo "==> Wrote $mutation_plist"
echo "==> Read-only: profile=$READONLY_PROFILE port=$READONLY_PORT"
echo "==> Mutation: profile=$MUTATION_PROFILE port=$MUTATION_PORT"
echo "==> Logs: $LOG_DIR"

if [[ "$LOAD_AFTER_WRITE" == "1" ]]; then
	user_domain="gui/$(id -u)"
	for plist in "$readonly_plist" "$mutation_plist"; do
		launchctl bootout "$user_domain" "$plist" >/dev/null 2>&1 || true
		launchctl bootstrap "$user_domain" "$plist"
	done
	echo "==> Loaded both agents with launchctl bootstrap $user_domain"
else
	echo "==> Next step:"
	echo "    launchctl bootstrap gui/$(id -u) $readonly_plist"
	echo "    launchctl bootstrap gui/$(id -u) $mutation_plist"
fi
