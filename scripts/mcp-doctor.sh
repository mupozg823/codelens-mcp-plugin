#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: bash scripts/mcp-doctor.sh [repo-root] [--strict]

Checks two things:
1. whether machine-readable CodeLens host attach is detectable
2. whether the configured transport behind that attach is actually reachable

Examples:
  bash scripts/mcp-doctor.sh .
  bash scripts/mcp-doctor.sh . --strict
EOF
}

STRICT=0
REPO_ROOT=""

while [[ $# -gt 0 ]]; do
	case "$1" in
	-h | --help)
		usage
		exit 0
		;;
	--strict)
		STRICT=1
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
	REPO_ROOT="."
fi
REPO_ROOT="$(cd -- "$REPO_ROOT" && pwd)"

declare -a CODELENS_CANDIDATES=()

add_candidate() {
	local candidate="${1:-}"
	if [[ -z "$candidate" || ! -x "$candidate" ]]; then
		return
	fi
	for existing in "${CODELENS_CANDIDATES[@]:-}"; do
		if [[ "$existing" == "$candidate" ]]; then
			return
		fi
	done
	CODELENS_CANDIDATES+=("$candidate")
}

add_candidate "${CODELENS_MCP_BIN:-}"
add_candidate "$(command -v codelens-mcp 2>/dev/null || true)"
add_candidate "$REPO_ROOT/target/release/codelens-mcp"
add_candidate "$REPO_ROOT/target/debug/codelens-mcp"
add_candidate "$HOME/.local/bin/codelens-mcp"
add_candidate "$HOME/.cargo/bin/codelens-mcp"

CODELENS_BIN=""
for candidate in "${CODELENS_CANDIDATES[@]:-}"; do
	if (
		cd "$REPO_ROOT"
		"$candidate" status --json --all >/dev/null 2>&1
	); then
		CODELENS_BIN="$candidate"
		break
	fi
done

if [[ -z "$CODELENS_BIN" ]]; then
	cat >&2 <<EOF
No usable codelens-mcp binary was found for \`status --json --all\`.
Tried:
$(printf '  - %s\n' "${CODELENS_CANDIDATES[@]:-<none>}")

Run: bash scripts/sync-local-bin.sh .
EOF
	exit 2
fi

STATUS_JSON="$(
	cd "$REPO_ROOT"
	"$CODELENS_BIN" status --json --all
)"

CODELENS_STATUS_JSON="$STATUS_JSON" python3 - "$REPO_ROOT" "$STRICT" "$CODELENS_BIN" <<'PY'
import json
import os
import shutil
import subprocess
import sys
import tomllib
import urllib.error
import urllib.request
from pathlib import Path

repo_root = Path(sys.argv[1])
strict = sys.argv[2] == "1"
codelens_bin = sys.argv[3]
payload = json.loads(os.environ["CODELENS_STATUS_JSON"])


def load_json_config(path: Path):
    data = json.loads(path.read_text())
    if isinstance(data.get("mcpServers"), dict):
        return data["mcpServers"].get("codelens")
    if isinstance(data.get("servers"), dict):
        return data["servers"].get("codelens")
    value = data.get("codelens")
    return value if isinstance(value, dict) else None


def load_toml_config(path: Path):
    data = tomllib.loads(path.read_text())
    mcp_servers = data.get("mcp_servers")
    if isinstance(mcp_servers, dict):
        value = mcp_servers.get("codelens")
        return value if isinstance(value, dict) else None
    return None


def parse_transport(path: Path, file_format: str):
    try:
        if file_format == "json":
            entry = load_json_config(path)
        elif file_format == "toml":
            entry = load_toml_config(path)
        else:
            return None, "unsupported machine-readable format"
    except FileNotFoundError:
        return None, "config file missing"
    except Exception as exc:
        return None, f"failed to parse config: {type(exc).__name__}"

    if not isinstance(entry, dict):
        return None, "CodeLens entry not found in config"
    if isinstance(entry.get("url"), str) and entry["url"].strip():
        return {
            "kind": "http",
            "value": entry["url"].strip(),
        }, None
    if isinstance(entry.get("command"), str) and entry["command"].strip():
        args = entry.get("args")
        if not isinstance(args, list):
            args = []
        return {
            "kind": "stdio",
            "value": entry["command"].strip(),
            "args": [str(item) for item in args],
            "config_path": str(path),
        }, None
    return None, "CodeLens entry is missing both url and command"


def check_http(url: str):
    request = urllib.request.Request(url, method="GET")
    try:
        with urllib.request.urlopen(request, timeout=1.2) as response:
            return True, f"HTTP {response.status}"
    except urllib.error.HTTPError as exc:
        return True, f"HTTP {exc.code}"
    except Exception as exc:
        return False, f"{type(exc).__name__}: {exc}"


def resolve_command(command: str, config_path: str):
    command_path = Path(command).expanduser()
    if "/" in command or str(command_path).startswith("."):
        if not command_path.is_absolute():
            command_path = (Path(config_path).parent / command_path).resolve()
        return str(command_path) if os.access(command_path, os.X_OK) else None
    return shutil.which(command)


def check_stdio(host: str, transport: dict):
    resolved = resolve_command(transport["value"], transport["config_path"])
    if not resolved:
        return False, f"command not found: {transport['value']}"

    basename = Path(resolved).name
    if basename == "codelens-mcp":
        try:
            completed = subprocess.run(
                [resolved, "status", "--json", host],
                cwd=repo_root,
                capture_output=True,
                text=True,
                timeout=1.5,
                check=False,
            )
        except Exception as exc:
            return False, f"failed to execute {resolved}: {type(exc).__name__}"
        if completed.returncode != 0:
            stderr = completed.stderr.strip() or completed.stdout.strip() or f"exit {completed.returncode}"
            return False, f"status subcommand failed: {stderr}"
        try:
            check_payload = json.loads(completed.stdout)
        except Exception as exc:
            return False, f"status output was not valid JSON: {type(exc).__name__}"
        hosts = check_payload.get("hosts") or []
        if not hosts or hosts[0].get("host") != host:
            return False, "status subcommand did not return the expected host payload"
        return True, f"resolved {resolved}"

    try:
        completed = subprocess.run(
            [resolved, "--help"],
            cwd=repo_root,
            capture_output=True,
            text=True,
            timeout=1.0,
            check=False,
        )
    except Exception as exc:
        return False, f"failed to execute {resolved}: {type(exc).__name__}"
    if completed.returncode == 0:
        return True, f"basic executable check only ({resolved})"
    return False, f"command exists but did not pass --help smoke check: {resolved}"


hard_statuses = {"invalid_json", "missing_codelens_entry", "missing_codelens_section"}
attached_prefixes = ("attached_exact", "attached_customized")

print("CodeLens MCP doctor")
print(f"Repo: {repo_root}")
print(f"Binary: {codelens_bin}")
print(f"Strict: {'yes' if strict else 'no'}")
print()

attached_count = 0
unconfigured_count = 0
issue_count = 0
issues = []

for host in payload.get("hosts", []):
    host_name = host.get("host", "unknown")
    files = host.get("files") or []
    machine_files = [f for f in files if f.get("format") in {"json", "toml"}]
    attached_files = [f for f in machine_files if str(f.get("status", "")).startswith("attached_")]
    bad_files = [f for f in machine_files if f.get("status") in hard_statuses]

    if attached_files:
        attached_count += 1
        active = attached_files[0]
        config_path = Path(active["path"]).expanduser()
        transport, parse_error = parse_transport(config_path, active.get("format", ""))
        if parse_error:
            issue_count += 1
            issues.append(f"{host_name}: {parse_error} ({config_path})")
            print(f"- {host_name}: ATTACHED via {config_path} [{active['status']}]")
            print(f"  transport: parse-failed ({parse_error})")
            continue

        if transport["kind"] == "http":
            ok, detail = check_http(transport["value"])
            transport_desc = f"http {transport['value']}"
        else:
            ok, detail = check_stdio(host_name, transport)
            transport_desc = f"stdio {transport['value']}"

        verdict = "OK" if ok else "FAIL"
        if not ok:
            issue_count += 1
            issues.append(f"{host_name}: {transport_desc} -> {detail}")

        print(f"- {host_name}: ATTACHED via {config_path} [{active['status']}]")
        print(f"  transport: {transport_desc}")
        print(f"  smoke: {verdict} ({detail})")
        continue

    if bad_files:
        issue_count += 1
        file = bad_files[0]
        issues.append(f"{host_name}: {file['path']} -> {file['message']}")
        print(f"- {host_name}: BROKEN attach metadata")
        print(f"  config: {file['path']} [{file['status']}]")
        print(f"  detail: {file['message']}")
        continue

    unconfigured_count += 1
    print(f"- {host_name}: not configured")

print()
print(
    f"Summary: attached={attached_count}, unconfigured={unconfigured_count}, issues={issue_count}"
)

if issues:
    print("Issues:")
    for issue in issues:
        print(f"- {issue}")

if strict and (issue_count > 0 or attached_count == 0):
    if attached_count == 0:
        print("Strict verdict: FAIL (no configured machine-readable attach found)")
    else:
        print("Strict verdict: FAIL")
    raise SystemExit(1)

print("Strict verdict: PASS" if strict else "Advisory verdict: PASS")
PY
