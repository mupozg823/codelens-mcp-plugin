#!/usr/bin/env python3
"""Collect CamelCase/PascalCase-heavy training pairs from local TS/Python projects.

Extracts exported symbols (functions, classes, interfaces, types) from TS/TSX/Python
files and generates training pairs with:
- query: derived from JSDoc/docstring or a generated natural-language description
- positive: CodeLens embedding format with identifier splitting

Usage:
    python collect_camelcase_data.py --projects /path/to/proj1 /path/to/proj2
"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
DEFAULT_OUTPUT = SCRIPT_DIR / "training_pairs_camelcase.jsonl"

SKIP_DIRS = {
    "node_modules",
    ".next",
    "__pycache__",
    "venv",
    "env",
    ".venv",
    "dist",
    "build",
    ".git",
    "target",
    ".tox",
    "coverage",
}


def split_identifier(name: str) -> str:
    if "_" in name:
        parts = [p for p in name.split("_") if p]
        expanded = []
        for part in parts:
            expanded.extend(_split_camel(part))
        return " ".join(w.lower() for w in expanded if w)
    return " ".join(w.lower() for w in _split_camel(name) if w)


def _split_camel(s: str) -> list[str]:
    parts = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", s)
    parts = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1 \2", parts)
    return parts.split()


def is_camelcase(name: str) -> bool:
    """Check if name has CamelCase or PascalCase pattern."""
    return bool(re.search(r"[a-z][A-Z]|^[A-Z][a-z].*[A-Z]", name))


def should_skip(path: Path) -> bool:
    return any(part in SKIP_DIRS for part in path.parts)


def extract_ts_symbols(filepath: Path) -> list[dict]:
    """Extract exported symbols from TypeScript/TSX files."""
    symbols = []
    try:
        content = filepath.read_text(errors="replace")
    except (OSError, UnicodeDecodeError):
        return symbols

    lines = content.split("\n")
    rel_path = filepath.name

    # Patterns for TS exports
    patterns = [
        # export function name(
        (r"export\s+(?:async\s+)?function\s+(\w+)", "function"),
        # export class name
        (r"export\s+(?:abstract\s+)?class\s+(\w+)", "class"),
        # export interface name
        (r"export\s+interface\s+(\w+)", "interface"),
        # export type name
        (r"export\s+type\s+(\w+)", "type"),
        # export const name =
        (r"export\s+const\s+(\w+)\s*[=:]", "const"),
        # export default function name
        (r"export\s+default\s+(?:async\s+)?function\s+(\w+)", "function"),
    ]

    for i, line in enumerate(lines):
        for pattern, kind in patterns:
            m = re.search(pattern, line)
            if m:
                name = m.group(1)
                if len(name) < 2:
                    continue
                # Get signature (this line + maybe next)
                sig = line.strip()
                if len(sig) > 120:
                    sig = sig[:120]

                # Try to find JSDoc above
                doc = _extract_jsdoc(lines, i)

                symbols.append(
                    {
                        "name": name,
                        "kind": kind,
                        "signature": sig,
                        "file": rel_path,
                        "doc": doc,
                        "is_camel": is_camelcase(name),
                    }
                )
                break

    return symbols


def _extract_jsdoc(lines: list[str], line_idx: int) -> str:
    """Extract JSDoc comment above a symbol definition."""
    if line_idx == 0:
        return ""

    # Look backwards for */ ending
    doc_lines = []
    for i in range(line_idx - 1, max(line_idx - 20, -1), -1):
        stripped = lines[i].strip()
        if stripped.endswith("*/"):
            doc_lines.insert(0, stripped)
            for j in range(i - 1, max(i - 20, -1), -1):
                s = lines[j].strip()
                doc_lines.insert(0, s)
                if s.startswith("/**") or s.startswith("/*"):
                    break
            break
        elif (
            stripped and not stripped.startswith("//") and not stripped.startswith("*")
        ):
            break

    if not doc_lines:
        return ""

    # Clean JSDoc
    text = " ".join(doc_lines)
    text = re.sub(r"/\*\*?\s*|\s*\*/", "", text)
    text = re.sub(r"\s*\*\s*", " ", text)
    text = re.sub(r"@\w+\s+\{[^}]*\}\s*\w*\s*-?\s*", "", text)
    text = re.sub(r"@\w+.*", "", text)
    text = text.strip()
    return text[:200] if text else ""


def extract_py_symbols(filepath: Path) -> list[dict]:
    """Extract classes and functions from Python files."""
    symbols = []
    try:
        content = filepath.read_text(errors="replace")
    except (OSError, UnicodeDecodeError):
        return symbols

    lines = content.split("\n")
    rel_path = filepath.name

    patterns = [
        (r"^(?:async\s+)?def\s+(\w+)\s*\(", "function"),
        (r"^class\s+(\w+)", "class"),
    ]

    for i, line in enumerate(lines):
        for pattern, kind in patterns:
            m = re.search(pattern, line)
            if m:
                name = m.group(1)
                if len(name) < 2 or name.startswith("__"):
                    continue

                sig = line.strip()
                if len(sig) > 120:
                    sig = sig[:120]

                # Extract docstring
                doc = _extract_pydoc(lines, i)

                symbols.append(
                    {
                        "name": name,
                        "kind": kind,
                        "signature": sig,
                        "file": rel_path,
                        "doc": doc,
                        "is_camel": is_camelcase(name),
                    }
                )
                break

    return symbols


def _extract_pydoc(lines: list[str], def_line: int) -> str:
    """Extract docstring after a def/class line."""
    if def_line + 1 >= len(lines):
        return ""

    # Look for triple-quote on next lines
    for i in range(def_line + 1, min(def_line + 3, len(lines))):
        stripped = lines[i].strip()
        if stripped.startswith('"""') or stripped.startswith("'''"):
            quote = stripped[:3]
            if stripped.count(quote) >= 2:
                # Single-line docstring
                return stripped.strip(quote).strip()[:200]
            # Multi-line: collect until closing
            doc_parts = [stripped.lstrip(quote)]
            for j in range(i + 1, min(i + 20, len(lines))):
                s = lines[j].strip()
                if quote in s:
                    doc_parts.append(s.rstrip(quote))
                    break
                doc_parts.append(s)
            return " ".join(doc_parts).strip()[:200]
        elif stripped and not stripped.startswith("#"):
            break

    return ""


def generate_query(symbol: dict) -> str:
    """Generate a natural-language query for the symbol."""
    if symbol["doc"]:
        return symbol["doc"]

    # Generate from name
    name = symbol["name"]
    split = split_identifier(name)
    kind = symbol["kind"]

    if kind == "class":
        return f"class that handles {split}"
    elif kind == "interface":
        return f"interface for {split}"
    elif kind == "type":
        return f"type definition for {split}"
    else:
        return (
            f"function that {split}s"
            if not split.endswith("s")
            else f"function that {split}"
        )


def build_codelens_text(symbol: dict) -> str:
    """Build CodeLens embedding format."""
    name = symbol["name"]
    split = split_identifier(name)
    return (
        f"{symbol['kind']} {name} ({split}) in {symbol['file']}: {symbol['signature']}"
    )


def collect_from_project(project_path: Path) -> list[dict]:
    """Collect symbols from a project directory."""
    symbols = []

    for filepath in project_path.rglob("*"):
        if should_skip(filepath):
            continue
        if filepath.suffix in {".ts", ".tsx"}:
            symbols.extend(extract_ts_symbols(filepath))
        elif filepath.suffix == ".py":
            symbols.extend(extract_py_symbols(filepath))

    return symbols


def main():
    parser = argparse.ArgumentParser(description="Collect CamelCase training data")
    parser.add_argument(
        "--projects",
        nargs="+",
        default=[
            "/Users/bagjaeseog/rg-family",
            "/Users/bagjaeseog/python-sdk",
            "/Users/bagjaeseog/opencode",
            "/Users/bagjaeseog/re pro",
            "/Users/bagjaeseog/깡깡벨퀴즈쇼",
        ],
    )
    parser.add_argument("--output", default=str(DEFAULT_OUTPUT))
    parser.add_argument(
        "--camelcase-only",
        action="store_true",
        help="Only include CamelCase/PascalCase symbols",
    )
    args = parser.parse_args()

    all_pairs = []
    seen_keys = set()

    for proj in args.projects:
        proj_path = Path(proj)
        if not proj_path.exists():
            print(f"SKIP (not found): {proj}")
            continue

        symbols = collect_from_project(proj_path)
        camel_count = sum(1 for s in symbols if s["is_camel"])
        print(f"{proj_path.name}: {len(symbols)} symbols ({camel_count} CamelCase)")

        for sym in symbols:
            if args.camelcase_only and not sym["is_camel"]:
                continue

            query = generate_query(sym)
            positive = build_codelens_text(sym)
            key = (query, positive)
            if key in seen_keys:
                continue
            seen_keys.add(key)

            all_pairs.append(
                {
                    "query": query,
                    "positive": positive,
                    "negative": "",
                    "source": proj_path.name,
                    "is_camel": sym["is_camel"],
                }
            )

    output_path = Path(args.output)
    with output_path.open("w") as f:
        for pair in all_pairs:
            f.write(json.dumps(pair, ensure_ascii=False) + "\n")

    camel_total = sum(1 for p in all_pairs if p["is_camel"])
    print(f"\nTotal: {len(all_pairs)} pairs ({camel_total} CamelCase)")
    print(f"Wrote to {output_path}")


if __name__ == "__main__":
    main()
