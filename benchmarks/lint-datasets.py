#!/usr/bin/env python3
"""Lint benchmark datasets for common hygiene issues."""

import argparse
import json
import re
import sys
from collections import Counter
from pathlib import Path


def duplicate_values(values: list[str]) -> list[str]:
    counts = Counter(values)
    return sorted(value for value, count in counts.items() if count > 1)


def looks_like_type_symbol(symbol: str) -> bool:
    return bool(symbol) and symbol[0].isupper() and "::" not in symbol


def looks_like_callable_symbol(symbol: str) -> bool:
    return "::" in symbol or (bool(symbol) and symbol[0].islower())


def query_expects_type(query_lower: str) -> bool:
    terms = set(re.findall(r"[a-z_]+", query_lower))
    if bool({"struct", "class", "interface", "enum", "trait"} & terms):
        return True
    return any(
        phrase in query_lower
        for phrase in (
            "type definition",
            "struct type",
            "class type",
            "which type is",
            "what type is",
            "which rust type is",
            "what rust type is",
        )
    )


def query_expects_callable(query_lower: str) -> bool:
    terms = set(re.findall(r"[a-z_]+", query_lower))
    return bool(
        {
            "constructor",
            "initializer",
            "helper",
            "handler",
            "entrypoint",
            "predicate",
            "builder",
            "method",
        }
        & terms
    )


def lint_dataset(dataset_path: str, project_root: str) -> list[dict]:
    """Validate a single dataset file. Returns list of issues."""
    issues = []
    text = Path(dataset_path).read_text()
    try:
        data = json.loads(text)
    except json.JSONDecodeError as e:
        issues.append({"level": "error", "msg": f"JSON parse error: {e}"})
        return issues

    # Handle wrapped format (schema_version + rows) or bare list
    if isinstance(data, dict) and "rows" in data:
        rows = data["rows"]
    elif isinstance(data, list):
        rows = data
    else:
        issues.append({"level": "error", "msg": f"Unknown format: {type(data)}"})
        return issues

    seen_queries: dict[str, int] = {}

    for i, row in enumerate(rows):
        # Check 1: expected_file_suffix exists on disk
        suffix = row.get("expected_file_suffix", "")
        if suffix:
            full_path = Path(project_root) / suffix
            if not full_path.exists():
                issues.append(
                    {
                        "level": "error",
                        "row": i,
                        "check": "file_exists",
                        "msg": f"Expected file not found: {suffix}",
                        "query": row.get("query", "")[:50],
                    }
                )

        # Check 2: expected_symbol not empty
        sym = row.get("expected_symbol", "")
        if not sym:
            issues.append(
                {
                    "level": "error",
                    "row": i,
                    "check": "symbol_present",
                    "msg": "Missing expected_symbol",
                }
            )

        # Check 3: negative_symbols should not contain expected_symbol
        negatives = row.get("negative_symbols", [])
        if sym and sym in negatives:
            issues.append(
                {
                    "level": "error",
                    "row": i,
                    "check": "negative_excludes_positive",
                    "msg": f"negative_symbols contains expected: {sym}",
                }
            )

        # Check 3b: negative_file_suffixes should not contain expected_file_suffix
        negative_suffixes = row.get("negative_file_suffixes", [])
        if suffix and suffix in negative_suffixes:
            issues.append(
                {
                    "level": "error",
                    "row": i,
                    "check": "negative_file_excludes_positive",
                    "msg": f"negative_file_suffixes contains expected: {suffix}",
                }
            )

        # Check 3c: duplicate negative entries usually indicate dataset drift
        duplicate_negative_symbols = duplicate_values(
            [value for value in negatives if isinstance(value, str)]
        )
        if duplicate_negative_symbols:
            issues.append(
                {
                    "level": "warning",
                    "row": i,
                    "check": "duplicate_negative_symbols",
                    "msg": "duplicate negative_symbols entries: "
                    + ", ".join(duplicate_negative_symbols[:3]),
                }
            )
        duplicate_negative_suffixes = duplicate_values(
            [value for value in negative_suffixes if isinstance(value, str)]
        )
        if duplicate_negative_suffixes:
            issues.append(
                {
                    "level": "warning",
                    "row": i,
                    "check": "duplicate_negative_file_suffixes",
                    "msg": "duplicate negative_file_suffixes entries: "
                    + ", ".join(duplicate_negative_suffixes[:3]),
                }
            )

        # Check 4: query not empty
        query = row.get("query", "")
        if not query.strip():
            issues.append(
                {
                    "level": "error",
                    "row": i,
                    "check": "query_present",
                    "msg": "Empty query",
                }
            )

        # Check 5: duplicate query detection (collected inline)
        if query:
            if query in seen_queries:
                issues.append(
                    {
                        "level": "warning",
                        "row": i,
                        "check": "duplicate_query",
                        "msg": f"Duplicate of row {seen_queries[query]}: {query[:50]}",
                    }
                )
            else:
                seen_queries[query] = i

        # Check 6: query intent should roughly match expected symbol class.
        query_lower = query.lower()
        if sym and query_expects_type(query_lower) and looks_like_callable_symbol(sym):
            issues.append(
                {
                    "level": "warning",
                    "row": i,
                    "check": "query_symbol_kind_mismatch",
                    "msg": f"query sounds type-like but expected symbol looks callable: {sym}",
                }
            )
        if (
            sym
            and query_expects_callable(query_lower)
            and not query_expects_type(query_lower)
            and looks_like_type_symbol(sym)
        ):
            issues.append(
                {
                    "level": "warning",
                    "row": i,
                    "check": "query_symbol_kind_mismatch",
                    "msg": f"query sounds callable but expected symbol looks type-like: {sym}",
                }
            )

    return issues


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Lint benchmark datasets for common hygiene issues."
    )
    parser.add_argument("--project", default=".", help="Project root directory")
    parser.add_argument(
        "--datasets",
        nargs="+",
        default=[
            "benchmarks/embedding-quality-dataset-self.json",
            "benchmarks/role-retrieval-dataset.json",
        ],
    )
    parser.add_argument(
        "--strict",
        action="store_true",
        help="Exit 1 on any warning (in addition to errors)",
    )
    args = parser.parse_args()

    project_root = str(Path(args.project).resolve())

    total_errors = 0
    total_warnings = 0

    for ds in args.datasets:
        ds_path = Path(ds)
        if not ds_path.exists():
            print(f"SKIP: {ds} not found")
            continue

        issues = lint_dataset(str(ds_path), project_root)
        errors = [i for i in issues if i["level"] == "error"]
        warnings = [i for i in issues if i["level"] == "warning"]
        total_errors += len(errors)
        total_warnings += len(warnings)

        # Count rows for the OK message
        try:
            data = json.loads(ds_path.read_text())
            rows = data["rows"] if isinstance(data, dict) and "rows" in data else data
            row_count = len(rows) if isinstance(rows, list) else 0
        except Exception:
            row_count = 0

        if issues:
            print(f"\n=== {ds} ({len(errors)} errors, {len(warnings)} warnings) ===")
            for issue in issues:
                print(
                    f"  [{issue['level'].upper():7}] row {issue.get('row', '?')}: "
                    f"{issue.get('check', 'format')} — {issue['msg']}"
                )
        else:
            print(f"  OK: {ds} ({row_count} rows)")

    print(f"\nTotal: {total_errors} errors, {total_warnings} warnings")
    if total_errors > 0 or (args.strict and total_warnings > 0):
        sys.exit(1)


if __name__ == "__main__":
    main()
