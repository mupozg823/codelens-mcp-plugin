#!/usr/bin/env python3
"""Compare embedding runtime pressure profiles.

Wraps embedding-runtime.py and captures wall time plus child RSS/CPU usage so
Apple Silicon safety defaults can be compared against aggressive settings.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("project_path", nargs="?", default=".")
    parser.add_argument(
        "--binary",
        default=os.environ.get(
            "CODELENS_BIN",
            os.path.join(
                os.path.dirname(__file__), "..", "target", "debug", "codelens-mcp"
            ),
        ),
    )
    parser.add_argument("--preset", default="balanced")
    parser.add_argument("--query", default="find code that manages embedding models")
    parser.add_argument("--ranked-query", default="embedding model and semantic search")
    parser.add_argument("--search-runs", type=int, default=1)
    parser.add_argument("--ranked-runs", type=int, default=1)
    parser.add_argument("--profiles", default="safe,aggressive")
    parser.add_argument("--isolated-copy", action="store_true")
    parser.add_argument("--keep-isolated-copy", action="store_true")
    parser.add_argument("--output", default="")
    return parser.parse_args()


ARGS = parse_args()
PROJECT = os.path.abspath(ARGS.project_path)
BIN = os.path.abspath(ARGS.binary)
RUNTIME_SCRIPT = Path(__file__).with_name("embedding-runtime.py")


def parse_time_output(stderr: str) -> dict:
    metrics = {}
    real_match = re.search(r"^\s*([0-9.]+)\s+real\s+([0-9.]+)\s+user\s+([0-9.]+)\s+sys", stderr, re.M)
    if real_match:
        metrics["real_s"] = float(real_match.group(1))
        metrics["user_s"] = float(real_match.group(2))
        metrics["sys_s"] = float(real_match.group(3))

    rss_match = re.search(r"^\s*([0-9]+)\s+maximum resident set size", stderr, re.M)
    if rss_match:
        metrics["max_rss_bytes"] = int(rss_match.group(1))
    else:
        rss_match = re.search(r"Maximum resident set size \(kbytes\):\s*([0-9]+)", stderr)
        if rss_match:
            metrics["max_rss_bytes"] = int(rss_match.group(1)) * 1024
    return metrics


def time_command(argv: list[str], env: dict[str, str]) -> dict:
    if shutil.which("/usr/bin/time"):
        system = platform.system()
        if system == "Darwin":
            time_argv = ["/usr/bin/time", "-l", *argv]
        else:
            time_argv = ["/usr/bin/time", "-v", *argv]
    else:
        time_argv = argv

    result = subprocess.run(
        time_argv,
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )

    payload = None
    stdout = result.stdout.strip()
    if stdout:
        try:
            payload = json.loads(stdout)
        except json.JSONDecodeError:
            try:
                payload = json.loads(stdout.splitlines()[-1])
            except json.JSONDecodeError:
                payload = None

    parsed = parse_time_output(result.stderr)
    return {
        "returncode": result.returncode,
        "payload": payload,
        "stderr": result.stderr.strip(),
        "time_metrics": parsed,
    }


def profile_env(name: str) -> dict[str, str]:
    env = os.environ.copy()
    env["CODELENS_BIN"] = BIN

    if name == "safe":
        for key in (
            "CODELENS_EMBED_THREADS",
            "CODELENS_EMBED_BATCH_SIZE",
            "OMP_NUM_THREADS",
            "VECLIB_MAXIMUM_THREADS",
            "OMP_WAIT_POLICY",
            "OMP_DYNAMIC",
            "TOKENIZERS_PARALLELISM",
        ):
            env.pop(key, None)
    elif name == "aggressive":
        cpu = str(os.cpu_count() or 1)
        env["CODELENS_EMBED_THREADS"] = cpu
        env["CODELENS_EMBED_BATCH_SIZE"] = "256"
        env["OMP_NUM_THREADS"] = cpu
        env["OMP_WAIT_POLICY"] = "ACTIVE"
        env["OMP_DYNAMIC"] = "FALSE"
        env["TOKENIZERS_PARALLELISM"] = "true"
        if platform.system() == "Darwin":
            env["VECLIB_MAXIMUM_THREADS"] = cpu
    else:
        raise SystemExit(f"unknown profile: {name}")
    return env


def run_profile(name: str) -> dict:
    argv = [
        sys.executable,
        str(RUNTIME_SCRIPT),
        PROJECT,
        "--binary",
        BIN,
        "--preset",
        ARGS.preset,
        "--query",
        ARGS.query,
        "--ranked-query",
        ARGS.ranked_query,
        "--search-runs",
        str(ARGS.search_runs),
        "--ranked-runs",
        str(ARGS.ranked_runs),
    ]
    if ARGS.isolated_copy:
        argv.append("--isolated-copy")
    if ARGS.keep_isolated_copy:
        argv.append("--keep-isolated-copy")

    result = time_command(argv, profile_env(name))
    if result["returncode"] != 0 or not isinstance(result["payload"], dict):
        raise SystemExit(
            f"profile={name} failed returncode={result['returncode']} stderr={result['stderr']}"
        )
    return {
        "profile": name,
        "time_metrics": result["time_metrics"],
        "runtime": result["payload"],
    }


def main():
    profiles = [p.strip() for p in ARGS.profiles.split(",") if p.strip()]
    runs = [run_profile(name) for name in profiles]

    output = {
        "project": PROJECT,
        "binary": BIN,
        "profiles": {run["profile"]: run for run in runs},
    }

    if ARGS.output:
        Path(ARGS.output).write_text(json.dumps(output, indent=2) + "\n")
    print(json.dumps(output, indent=2))


if __name__ == "__main__":
    main()
