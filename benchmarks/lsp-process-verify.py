#!/usr/bin/env python3
"""Verify that `prepare_harness_session` with `CODELENS_LSP_AUTO=true`
actually spawns the LSP servers it reports under `prewarm_fired`.

The auto-attach telemetry is trustworthy only if those OS-level
processes exist after the bootstrap call. Run this alongside the
auto-attach bench numbers (`docs/benchmarks.md §1c`) to confirm the
telemetry is not reporting success while the servers are silently
absent.

Usage:
    python3 benchmarks/lsp-process-verify.py

Exits 0 when every language in `prewarm_fired` has at least one
matching `pgrep` hit after a short settle delay, 1 otherwise.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import benchmark_runtime_common as rc  # noqa: E402


REPO_ROOT = Path(__file__).resolve().parent.parent

# `prewarm_fired` uses the LSP recipe language ids; map them to
# process-name fragments that the server invokes under.
PROCESS_MARKERS: dict[str, list[str]] = {
    "python": ["pyright-langserver"],
    "typescript": ["typescript-language-server"],
    "rust": ["rust-analyzer"],
    "go": ["gopls"],
    "java": ["jdtls"],
    "kotlin": ["kotlin-language-server"],
    "ruby": ["solargraph"],
    "shellscript": ["bash-language-server"],
}


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--binary",
        default=str(REPO_ROOT / "target" / "release" / "codelens-mcp"),
    )
    p.add_argument("--project", default=str(REPO_ROOT))
    p.add_argument("--preset", default="full")
    p.add_argument(
        "--settle",
        type=int,
        default=3,
        help="Seconds to wait after prepare_harness_session before sampling pgrep.",
    )
    return p.parse_args()


def list_lsp_procs(markers: list[str]) -> list[str]:
    """Return `pgrep -laf` lines matching *only* an LSP binary name.

    Excludes this script itself, which names the binaries in its
    argv and would otherwise match its own regex.
    """
    if not markers:
        return []
    rx = "|".join(re.escape(m) for m in markers)
    result = subprocess.run(
        ["pgrep", "-laf", rx],
        capture_output=True,
        text=True,
    )
    if result.returncode not in (0, 1):
        return []
    own_pid = str(os.getpid())
    rows = []
    for line in result.stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        pid = line.split(" ", 1)[0]
        # Skip our own pgrep-triggering python/lsp-process-verify.py invocation.
        if pid == own_pid:
            continue
        if "lsp-process-verify" in line:
            continue
        rows.append(line)
    return rows


def extract_data(response: dict) -> dict:
    """See `benchmarks/lsp-boost-http-matrix.py::extract_data` — same
    two-shape handling (wrapped `.data` vs flattened workflow root).
    """
    if not isinstance(response, dict):
        return {}
    result = response.get("result")
    if not isinstance(result, dict):
        return {}
    structured = result.get("structuredContent")
    if isinstance(structured, dict):
        data = structured.get("data")
        if isinstance(data, dict):
            return data
        return structured
    content = result.get("content")
    if isinstance(content, list) and content:
        try:
            parsed = __import__("json").loads(content[0].get("text", "{}"))
        except Exception:
            return {}
        if isinstance(parsed, dict):
            data = parsed.get("data")
            if isinstance(data, dict):
                return data
            return parsed
    return {}


def main() -> int:
    args = parse_args()
    os.environ["CODELENS_LSP_AUTO"] = "true"
    base_url, _port, proc = rc.start_http_daemon(
        args.binary, args.project, preset=args.preset
    )
    if not base_url:
        rc.stop_http_daemon(proc)
        raise SystemExit("HTTP daemon failed to start")

    try:
        session_id, _p, _h = rc.initialize_http_session(base_url, timeout_seconds=20)
        # Snapshot BEFORE so the diff makes it clear which procs this
        # run actually spawned.
        before = list_lsp_procs(
            list({m for ms in PROCESS_MARKERS.values() for m in ms})
        )

        resp = rc.mcp_http_tool_call(
            base_url,
            "prepare_harness_session",
            {},
            request_id=1,
            session_id=session_id,
            timeout_seconds=30,
        )
        data = extract_data(resp)
        auto = data.get("lsp_auto_attach", {}) if isinstance(data, dict) else {}
        prewarm = auto.get("prewarm_fired", []) if isinstance(auto, dict) else []

        time.sleep(args.settle)

        after_by_lang = {
            lang: list_lsp_procs(PROCESS_MARKERS.get(lang, [])) for lang in prewarm
        }

        print("prewarm_fired reported:", prewarm)
        print()
        print(f"Before prepare_harness_session ({len(before)} LSP procs):")
        for line in before:
            print(" ", line)
        print()
        print(f"After prepare_harness_session (+{args.settle}s):")
        missing = []
        for lang, lines in after_by_lang.items():
            print(f"  [{lang}]")
            if not lines:
                print(f"    MISSING — no matching LSP process for {lang}")
                missing.append(lang)
            else:
                for line in lines:
                    print(f"    {line}")

        if missing:
            print()
            print(
                "FAIL: prewarm_fired reported",
                missing,
                "but no matching LSP process was observed.",
            )
            return 1
        print()
        print("OK: every language in prewarm_fired has a live LSP process.")
        return 0
    finally:
        rc.stop_http_daemon(proc)


if __name__ == "__main__":
    raise SystemExit(main())
