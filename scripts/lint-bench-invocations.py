#!/usr/bin/env python3
"""Lint cargo bench invocations to require explicit target selection.

Catches the 2026-05 regression where ``.github/workflows/benchmark.yml``
omitted ``--benches`` / ``--bench <name>``, causing cargo bench to invoke
the lib + integration test binaries through the std test runner. The std
runner rejects criterion's ``--save-baseline`` / ``--baseline`` args with
``error: Unrecognized option: 'save-baseline'``, crashing the Benchmark
workflow on every push for 8 consecutive commits before the root cause
was diagnosed (fix in commit 10ac00d6).

Scans ``.github/workflows/`` and ``scripts/`` for ``cargo bench``
invocations. Each must include either ``--benches`` (all [[bench]]
targets) or ``--bench <name>`` (specific target).

Exit codes:
- 0 → all invocations specify a target selector
- 1 → at least one invocation is missing the selector
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
SEARCH_DIRS = (".github/workflows", "scripts")
CARGO_BENCH_RE = re.compile(r"\bcargo\s+bench\b")
TARGET_SELECTOR_RE = re.compile(r"--bench(?:es)?\b")
SELF_NAME = Path(__file__).name


def find_invocations() -> list[tuple[Path, int, str]]:
    hits: list[tuple[Path, int, str]] = []
    for d in SEARCH_DIRS:
        root = REPO_ROOT / d
        if not root.exists():
            continue
        for path in root.rglob("*"):
            if not path.is_file() or path.name == SELF_NAME:
                continue
            try:
                text = path.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            for i, line in enumerate(text.splitlines(), start=1):
                # Skip comment lines (yaml/python/shell `#`). This avoids
                # false positives when the lint's own incident description
                # appears inside a workflow comment block.
                if line.lstrip().startswith("#"):
                    continue
                if CARGO_BENCH_RE.search(line):
                    hits.append((path.relative_to(REPO_ROOT), i, line.strip()))
    return hits


def main() -> int:
    hits = find_invocations()
    if not hits:
        print("[lint-bench] no `cargo bench` invocations found", file=sys.stderr)
        return 0

    missing = {(p, n) for p, n, t in hits if not TARGET_SELECTOR_RE.search(t)}

    print(
        f"[lint-bench] scanned {len(hits)} `cargo bench` line(s); "
        f"{len(missing)} missing target selector"
    )
    for path, lineno, text in hits:
        status = "MISSING" if (path, lineno) in missing else "ok"
        print(f"  [{status:>7s}] {path}:{lineno}: {text}")

    if missing:
        print(
            "\nERROR: `cargo bench` without `--bench <name>` or `--benches` invokes\n"
            "the lib + integration test binaries through the std runner, which\n"
            "rejects criterion's `--save-baseline` / `--baseline` args. See commit\n"
            "10ac00d6 for the original incident.",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
