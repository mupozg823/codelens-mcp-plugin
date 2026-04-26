#!/usr/bin/env python3
"""Lint that the four tree-sitter refactor handlers keep their honesty surfaces.

Per docs/design/refactor-backend-honesty.md, every refactor handler that
has a tree-sitter fallback (i.e. matches `SemanticEditBackendSelection::TreeSitter`
in its backend-select match) must emit:

  - a ``tree_sitter_caveats`` array in the response payload
  - a ``degraded_reason`` field in the response payload
  - a call to ``degraded_meta(`` (not ``success_meta``) when returning the meta

This catches future PRs that add a new ``refactor_*`` tool with a tree-sitter
fallback but forget the honesty pattern, masking heuristic limits behind a
real-LSP-shaped response.

Exit codes:
- 0 → all four handlers carry the three honesty surfaces
- 1 → at least one handler is missing one or more surfaces
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
TARGET = REPO_ROOT / "crates/codelens-mcp/src/tools/composite.rs"

REQUIRED_HANDLERS = (
    "refactor_extract_function",
    "refactor_inline_function",
    "refactor_move_to_file",
    "refactor_change_signature",
)
REQUIRED_TOKENS = (
    "tree_sitter_caveats",
    "degraded_reason",
    "degraded_meta(",
)
TREE_SITTER_FALLBACK_MARKER = "SemanticEditBackendSelection::TreeSitter"


def extract_handler_body(source: str, fn_name: str) -> str | None:
    """Return the source of `pub fn fn_name(...)` from opening `{` to its
    matching closing `}`. Returns None if the function is not found."""
    pat = re.compile(
        rf"\bpub\s+fn\s+{re.escape(fn_name)}\b\s*\([^)]*\)\s*->\s*[^{{]+{{"
    )
    m = pat.search(source)
    if not m:
        return None
    start = m.end() - 1
    depth = 0
    for i in range(start, len(source)):
        ch = source[i]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return source[start : i + 1]
    return None


def check_handler(source: str, fn_name: str) -> list[str]:
    body = extract_handler_body(source, fn_name)
    if body is None:
        return [f"  {fn_name}: handler not found in composite.rs"]
    if TREE_SITTER_FALLBACK_MARKER not in body:
        return []  # no tree-sitter path → policy doesn't apply
    missing = [tok for tok in REQUIRED_TOKENS if tok not in body]
    if missing:
        return [f"  {fn_name}: missing {sorted(missing)}"]
    return []


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", default=str(TARGET))
    args = parser.parse_args()

    target = Path(args.target)
    if not target.exists():
        print(f"[lint-refactor-honesty] target file missing: {target}", file=sys.stderr)
        return 1
    source = target.read_text()

    errors: list[str] = []
    for fn_name in REQUIRED_HANDLERS:
        errors.extend(check_handler(source, fn_name))

    if errors:
        print(
            "[lint-refactor-honesty] tree-sitter refactor honesty surface missing:",
            file=sys.stderr,
        )
        for line in errors:
            print(line, file=sys.stderr)
        print(
            "see docs/design/refactor-backend-honesty.md for the required surfaces.",
            file=sys.stderr,
        )
        return 1

    print(
        f"[lint-refactor-honesty] OK — all {len(REQUIRED_HANDLERS)} handlers carry the honesty surfaces"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
