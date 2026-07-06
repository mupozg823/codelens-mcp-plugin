#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

# --- How to run ---
# 1. Install uv (if not installed):
#      curl -LsSf https://astral.sh/uv/install.sh | sh
# 2. Run from a source build:
#      uv run scripts/smoke-clean-quickstart.py --binary target/debug/codelens-mcp --model-root crates/codelens-engine/models
# 3. Run from an extracted release archive:
#      python3 scripts/smoke-clean-quickstart.py --binary ./codelens-mcp --model-root .
# ------------------

from __future__ import annotations

import argparse
import json
import tempfile
from pathlib import Path
from typing import Final

from quickstart_smoke_contract import QuickstartSmokeError, QuickstartSummary
from quickstart_smoke_archive import run_archive_smoke
from quickstart_smoke_runner import run_smoke


DEFAULT_TIMEOUT_SECONDS: Final[int] = 180


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Clean quickstart smoke for install -> status -> index -> retrieve."
    )
    parser.add_argument("--archive", help="Path to a release archive to extract and smoke")
    parser.add_argument("--binary", help="Path to codelens-mcp")
    parser.add_argument(
        "--model-root",
        help="Root containing models/codesearch, codesearch, or the model files",
    )
    parser.add_argument("--timeout", type=int, default=DEFAULT_TIMEOUT_SECONDS)
    parser.add_argument("--keep-temp", action="store_true")
    parser.add_argument(
        "--use-model-env",
        action="store_true",
        help="Set CODELENS_MODEL_DIR instead of proving executable-sidecar discovery",
    )
    parser.add_argument(
        "--homebrew-layout",
        action="store_true",
        help="With --binary, install into a Homebrew-style Cellar prefix before smoking",
    )
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def print_summary(summary: QuickstartSummary, emit_json: bool) -> None:
    if emit_json:
        print(json.dumps(summary.to_json(), indent=2))
        return
    print(summary.render())


def main() -> None:
    args = parse_args()
    if bool(args.archive) == bool(args.binary):
        raise SystemExit("provide exactly one of --archive or --binary")
    if args.binary and not args.model_root:
        raise SystemExit("--model-root is required with --binary")
    if args.archive and args.homebrew_layout:
        raise SystemExit("--homebrew-layout is only valid with --binary")
    try:
        if args.keep_temp:
            root = Path(tempfile.mkdtemp(prefix="codelens-clean-quickstart."))
            summary = run_from_args(args, root)
        else:
            with tempfile.TemporaryDirectory(prefix="codelens-clean-quickstart.") as root:
                summary = run_from_args(args, Path(root))
    except QuickstartSmokeError as error:
        raise SystemExit(str(error)) from error
    print_summary(summary, args.json)


def run_from_args(args: argparse.Namespace, root: Path) -> QuickstartSummary:
    if args.archive:
        archive = Path(args.archive).expanduser().resolve()
        if not archive.is_file():
            raise SystemExit(f"archive not found: {archive}")
        return run_archive_smoke(
            archive,
            root,
            args.timeout,
            use_model_env=args.use_model_env,
        )
    binary = Path(args.binary).expanduser().resolve()
    model_root = Path(args.model_root).expanduser().resolve()
    if not binary.is_file():
        raise SystemExit(f"binary not found: {binary}")
    if not model_root.exists():
        raise SystemExit(f"model root not found: {model_root}")
    return run_smoke(
        binary,
        model_root,
        root,
        args.timeout,
        use_model_env=args.use_model_env,
        homebrew_layout=args.homebrew_layout,
    )


if __name__ == "__main__":
    main()
