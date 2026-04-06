#!/usr/bin/env python3
"""Convert CodeSearchNet pairs to CodeLens embedding format (v2).

Improvements over v1:
- Better Java regex: handles generics (QueryResults<?>), annotations (@Override)
- JS anonymous functions: extract name from docstring/query when code has no name
- Ruby: handle ? and ! method suffixes
- Identifier splitting: CamelCase/snake_case → space-separated words
- Go: handle method receivers

Output format matches Rust build_embedding_text():
  {kind} {name} ({split_name}) in {lang}_project: {signature}
"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
DEFAULT_INPUT = SCRIPT_DIR / "codesearchnet_pairs.jsonl"
DEFAULT_OUTPUT = SCRIPT_DIR / "codesearchnet_codelens_v2.jsonl"


def split_identifier(name: str) -> str:
    """Split CamelCase and snake_case into space-separated words."""
    # snake_case → words
    if "_" in name:
        parts = [p for p in name.split("_") if p]
        # Also split any CamelCase within each part
        expanded = []
        for part in parts:
            expanded.extend(_split_camel(part))
        return " ".join(w.lower() for w in expanded if w)

    # CamelCase → words
    return " ".join(w.lower() for w in _split_camel(name) if w)


def _split_camel(s: str) -> list[str]:
    """Split CamelCase: 'parseHTTPResponse' → ['parse', 'HTTP', 'Response']."""
    # Insert boundary before uppercase following lowercase or before uppercase followed by lowercase
    parts = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", s)
    parts = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1 \2", parts)
    return parts.split()


def extract_signature(code: str, lang: str, max_len: int = 120) -> str:
    """Extract the first line / signature from code."""
    first_line = code.strip().split("\n")[0].strip()
    if len(first_line) > max_len:
        first_line = first_line[:max_len]
    return first_line


def extract_name_python(code: str) -> str | None:
    m = re.search(r"def\s+(\w+)", code)
    return m.group(1) if m else None


def extract_name_javascript(code: str) -> str | None:
    # Named function
    m = re.search(r"function\s+(\w+)", code)
    if m:
        return m.group(1)
    # Variable assignment: const/let/var name = function/arrow
    m = re.search(r"(?:const|let|var)\s+(\w+)\s*=", code)
    if m:
        return m.group(1)
    # Method shorthand: name(params) {
    m = re.search(r"^(\w+)\s*\(", code.strip())
    if m:
        return m.group(1)
    # prototype: Foo.prototype.bar
    m = re.search(r"\.prototype\.(\w+)", code)
    if m:
        return m.group(1)
    return None


def extract_name_java(code: str) -> str | None:
    # Strip leading annotations
    cleaned = re.sub(r"@\w+(?:\([^)]*\))?\s*", "", code)
    # Handle generics, arrays, etc in return type
    m = re.search(
        r"(?:public|private|protected|static|final|abstract|synchronized|native|\s)+"
        r"(?:[\w<>\[\]?,.\s]+)\s+(\w+)\s*\(",
        cleaned,
    )
    if m:
        return m.group(1)
    # Constructor or simple method without access modifier
    m = re.search(r"(\w+)\s*\(", cleaned.strip())
    if m:
        name = m.group(1)
        # Filter out keywords
        if name not in {
            "if",
            "for",
            "while",
            "switch",
            "catch",
            "return",
            "new",
            "throw",
        }:
            return name
    return None


def extract_name_go(code: str) -> str | None:
    m = re.search(r"func\s+(?:\([^)]+\)\s+)?(\w+)", code)
    return m.group(1) if m else None


def extract_name_ruby(code: str) -> str | None:
    m = re.search(r"def\s+([\w?!]+)", code)
    return m.group(1) if m else None


EXTRACTORS = {
    "python": extract_name_python,
    "javascript": extract_name_javascript,
    "java": extract_name_java,
    "go": extract_name_go,
    "ruby": extract_name_ruby,
}


def infer_name_from_query(query: str, lang: str) -> str | None:
    """Try to extract a function name from the docstring/query text."""
    # Look for patterns like "Return X", "Calculate Y" → derive a plausible name
    # Or look for explicit function references
    m = re.search(r"`(\w+)`", query)
    if m:
        return m.group(1)
    m = re.search(r"@function\s+(\w+)", query)
    if m:
        return m.group(1)
    m = re.search(r"@method\s+(\w+)", query)
    if m:
        return m.group(1)
    return None


def build_codelens_text(name: str, lang: str, signature: str) -> str:
    """Build CodeLens embedding text format."""
    split_name = split_identifier(name)
    kind = "function"
    file_ctx = f"{lang}_project"
    return f"{kind} {name} ({split_name}) in {file_ctx}: {signature}"


def convert_row(obj: dict) -> dict | None:
    """Convert a CSN row to CodeLens format. Returns None if extraction fails."""
    code = obj.get("positive", "")
    query = obj.get("query", "")
    lang = obj.get("language", "")

    if not code or not query or not lang:
        return None

    extractor = EXTRACTORS.get(lang)
    if not extractor:
        return None

    name = extractor(code)

    # Fallback: try to infer from query
    if name is None:
        name = infer_name_from_query(query, lang)

    if name is None:
        return None

    # Skip very short or meaningless names
    if len(name) < 2 or name in {"_", "__"}:
        return None

    signature = extract_signature(code, lang)
    positive_text = build_codelens_text(name, lang, signature)

    return {
        "query": query,
        "positive": positive_text,
        "language": lang,
        "function_name": name,
        "split_name": split_identifier(name),
    }


def main():
    parser = argparse.ArgumentParser(description="Convert CSN to CodeLens format v2")
    parser.add_argument("--input", default=str(DEFAULT_INPUT))
    parser.add_argument("--output", default=str(DEFAULT_OUTPUT))
    args = parser.parse_args()

    input_path = Path(args.input)
    output_path = Path(args.output)

    stats = {"total": 0, "converted": 0, "failed": 0, "by_lang": {}}

    with input_path.open() as fin, output_path.open("w") as fout:
        for line in fin:
            line = line.strip()
            if not line:
                continue
            stats["total"] += 1
            obj = json.loads(line)
            lang = obj.get("language", "unknown")

            result = convert_row(obj)
            if result:
                fout.write(json.dumps(result, ensure_ascii=False) + "\n")
                stats["converted"] += 1
                stats["by_lang"][lang] = stats["by_lang"].get(lang, 0) + 1
            else:
                stats["failed"] += 1

    print(json.dumps(stats, indent=2))
    print(f"\nWrote {stats['converted']} rows to {output_path}")
    print(f"Conversion rate: {stats['converted']/stats['total']*100:.1f}%")


if __name__ == "__main__":
    main()
