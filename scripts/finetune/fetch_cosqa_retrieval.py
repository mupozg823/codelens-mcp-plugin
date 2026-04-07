#!/usr/bin/env python3
"""Fetch and convert official CoSQA retrieval data into CodeLens runtime format."""

from __future__ import annotations

import argparse
import json
import re
import urllib.request
from collections import Counter
from pathlib import Path

from build_runtime_training_pipeline import (
    LANG_PATTERNS,
    build_pair,
    build_runtime_positive,
    first_line,
    normalize_space,
)


SCRIPT_DIR = Path(__file__).parent
DEFAULT_OUTPUT_DIR = SCRIPT_DIR / "external" / "cosqa"
OFFICIAL_BASE = (
    "https://raw.githubusercontent.com/Jun-jie-Huang/CoCLR/master/data/search"
)
FILES = {
    "train": "cosqa-retrieval-train-19604.json",
    "dev": "cosqa-retrieval-dev-500.json",
    "test": "cosqa-retrieval-test-500.json",
}


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", default=str(DEFAULT_OUTPUT_DIR))
    parser.add_argument("--skip-test", action="store_true")
    return parser.parse_args()


def download_json(url: str) -> list[dict]:
    with urllib.request.urlopen(url, timeout=60) as response:
        return json.load(response)


def infer_language(code: str) -> str:
    for language, pattern in LANG_PATTERNS.items():
        if re.search(pattern, code):
            return language
    return "python"


def extract_func_name(code: str, language: str) -> str | None:
    pattern = LANG_PATTERNS.get(language)
    if not pattern:
        return None
    match = re.search(pattern, code)
    if not match:
        return None
    for idx in range(1, (match.lastindex or 0) + 1):
        value = match.group(idx)
        if value:
            return value
    return None


def extract_code_doc_hint(code: str, language: str) -> str:
    lines = code.splitlines()[1:12]
    for line in lines:
        stripped = line.strip().strip('"').strip("'")
        if not stripped:
            continue
        if language == "python" and (
            stripped.startswith('"""')
            or stripped.startswith("'''")
            or stripped.startswith("#")
        ):
            return normalize_space(stripped.strip("#").strip('"').strip("'"))
        if stripped.startswith("//") or stripped.startswith("/*") or stripped.startswith("*"):
            return normalize_space(
                stripped.lstrip("/").lstrip("*").strip()
            )
    return ""


def convert_rows(rows: list[dict], split: str) -> list[dict]:
    converted = []
    for obj in rows:
        query = obj.get("doc") or obj.get("query") or ""
        code = obj.get("code") or ""
        language = infer_language(code)
        name = extract_func_name(code, language)
        if not name:
            continue
        positive = build_runtime_positive(
            name=name,
            kind="function",
            signature=first_line(code),
            file_path="",
            doc_hint=extract_code_doc_hint(code, language),
        )
        pair = build_pair(
            query,
            positive,
            language=language,
            source=f"cosqa_retrieval_{split}",
            query_type="natural_language",
        )
        if pair is None:
            continue
        converted.append(pair)
    return converted


def write_json(path: Path, payload: list[dict]) -> None:
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def write_jsonl(path: Path, rows: list[dict]) -> None:
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row, ensure_ascii=False) + "\n")


def summarize(name: str, rows: list[dict]) -> None:
    print(
        json.dumps(
            {
                "split": name,
                "count": len(rows),
                "languages": dict(sorted(Counter(row["language"] for row in rows).items())),
            },
            ensure_ascii=False,
        )
    )


def main():
    args = parse_args()
    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    for split, filename in FILES.items():
        if split == "test" and args.skip_test:
            continue
        raw_rows = download_json(f"{OFFICIAL_BASE}/{filename}")
        converted = convert_rows(raw_rows, split)
        write_json(output_dir / filename, raw_rows)
        write_jsonl(output_dir / filename.replace(".json", "-runtime.jsonl"), converted)
        summarize(split, converted)


if __name__ == "__main__":
    main()
