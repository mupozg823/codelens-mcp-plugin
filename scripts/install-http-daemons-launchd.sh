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
  --rerank VALUE              CODELENS_RERANK for both daemons (default: 0)
  --run-at-load               add RunAtLoad=true to generated plists
  --load                      bootstrap generated plists after writing
  --no-build                  reuse an existing http-capable binary at --bin-path
  --print-only                print both plists to stdout instead of writing them
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
RERANK_VALUE="0"
RUN_AT_LOAD=1
LOAD_AFTER_WRITE=0
NO_BUILD=0
PRINT_ONLY=0

is_int_in_range() {
	local value="$1"
	local min="$2"
	local max="$3"
	[[ "$value" =~ ^[0-9]+$ ]] || return 1
	((value >= min && value <= max))
}

xml_escape() {
	python3 - "$1" <<'PY'
import html
import sys

print(html.escape(sys.argv[1], quote=True))
PY
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
	--rerank)
		RERANK_VALUE="${2:-}"
		shift 2
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

mkdir -p "$(dirname "$BIN_PATH")"
LOG_DIR="$REPO_ROOT/.codelens/reports/launchd"
mkdir -p "$LOG_DIR"

if [[ "$NO_BUILD" == "0" ]]; then
	echo "==> Building codelens-mcp with http feature from $REPO_ROOT"
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
		cargo build -p codelens-mcp --release --features http
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
	label_xml="$(xml_escape "$label")"
	bin_xml="$(xml_escape "$BIN_PATH")"
	repo_xml="$(xml_escape "$REPO_ROOT")"
	stdout_xml="$(xml_escape "$stdout_path")"
	stderr_xml="$(xml_escape "$stderr_path")"

	mkdir -p "$(dirname "$plist_path")"
	cat >"$plist_path" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>${label_xml}</string>
  <key>ProgramArguments</key>
  <array>
    <string>${bin_xml}</string>
    <string>${repo_xml}</string>
    <string>--transport</string>
    <string>http</string>
    <string>--profile</string>
    <string>${profile}</string>
    <string>--daemon-mode</string>
    <string>${daemon_mode}</string>
    <string>--port</string>
    <string>${port}</string>
  </array>
  <key>WorkingDirectory</key>
  <string>${repo_xml}</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>CODELENS_LOG</key>
    <string>${log_level}</string>
    <key>CODELENS_EFFORT_LEVEL</key>
    <string>${EFFORT_LEVEL}</string>
    <key>CODELENS_RERANK</key>
    <string>${RERANK_VALUE}</string>
  </dict>
  <key>KeepAlive</key>
  <true/>
${run_at_load_xml}  <key>StandardOutPath</key>
  <string>${stdout_xml}</string>
  <key>StandardErrorPath</key>
  <string>${stderr_xml}</string>
</dict>
</plist>
EOF

	if command -v plutil >/dev/null 2>&1; then
		plutil -lint "$plist_path" >/dev/null
	fi
}

update_host_attach_config() {
	local config_path="$REPO_ROOT/.codelens/config.json"
	local readonly_url="http://127.0.0.1:${READONLY_PORT}/mcp"
	local mutation_url="http://127.0.0.1:${MUTATION_PORT}/mcp"

	mkdir -p "$(dirname "$config_path")"
	python3 - "$config_path" "$readonly_url" "$mutation_url" <<'PY'
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
PY
	echo "==> Updated host attach overrides in $config_path"
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
	exit 0
fi

create_plist "$readonly_label" "$READONLY_PROFILE" "read-only" "$READONLY_PORT" "$READONLY_LOG_LEVEL" "$readonly_stdout" "$readonly_stderr" "$readonly_plist"
create_plist "$mutation_label" "$MUTATION_PROFILE" "mutation-enabled" "$MUTATION_PORT" "$MUTATION_LOG_LEVEL" "$mutation_stdout" "$mutation_stderr" "$mutation_plist"
update_host_attach_config

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
