#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

# --- How to run ---
# 1. Install uv (if not installed):
#      curl -LsSf https://astral.sh/uv/install.sh | sh
# 2. Run directly:
#      uv run benchmarks/embedding-index-lifecycle.py . --binary target/debug/codelens-mcp
# 3. CI can also run it with system Python:
#      python3 benchmarks/embedding-index-lifecycle.py . --binary target/debug/codelens-mcp
# ------------------

"""CLI wrapper for the CodeLens semantic index lifecycle benchmark."""

from __future__ import annotations

import argparse
from pathlib import Path

from embedding_index_lifecycle_lib import (
    DEFAULT_TIMEOUT_SECONDS,
    CliArgs,
    IndexLifecycleError,
    default_output_path,
    run_benchmark,
)


def parse_args() -> CliArgs:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project", nargs="?", default=".")
    parser.add_argument("--binary", required=True, help="Path to codelens-mcp")
    parser.add_argument("--output", default="", help="Artifact path; defaults under /tmp")
    parser.add_argument("--timeout", type=int, default=DEFAULT_TIMEOUT_SECONDS)
    parser.add_argument("--keep-worktree", action="store_true")
    namespace = parser.parse_args()
    output = Path(namespace.output).expanduser() if namespace.output else default_output_path()
    return CliArgs(
        project=Path(namespace.project).expanduser().resolve(),
        binary=Path(namespace.binary).expanduser().resolve(),
        output=output.resolve(),
        timeout=namespace.timeout,
        keep_worktree=namespace.keep_worktree,
    )


def main() -> None:
    args = parse_args()
    if not args.project.is_dir():
        raise SystemExit(f"project directory not found: {args.project}")
    if not args.binary.is_file():
        raise SystemExit(f"binary not found: {args.binary}")
    try:
        summary = run_benchmark(args)
    except IndexLifecycleError as error:
        raise SystemExit(str(error)) from error
    print(f"index_lifecycle_artifact={summary.output}")
    print(f"cold_ms={summary.cold_ms} warm_ms={summary.warm_ms}")
    if summary.worktree is not None:
        print(f"benchmark_worktree={summary.worktree}")


if __name__ == "__main__":
    main()
