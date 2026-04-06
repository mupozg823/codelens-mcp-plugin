#!/usr/bin/env python3
"""Download original CodeSearchNet and convert to EXACT runtime embedding format.

Runtime format (from embedding.rs build_embedding_text):
  "{kind} {name} ({split_name}) in {file_path}: {signature}"

Data source: code-search-net/code_search_net (original, NOT sentence-transformers version)
  - Has func_path_in_repository (REAL file paths like "imcut/pycut.py")
  - Has func_name, func_documentation_string, func_code_string, language

NO local data. Internet high-quality data ONLY.
"""

import json
import re
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
OUTPUT = SCRIPT_DIR / "csn_runtime_format.jsonl"

MAX_PER_LANG = 12000
MIN_QUERY_LEN = 15
MAX_QUERY_LEN = 300
LANGUAGES = ["python", "javascript", "go", "java", "ruby", "php"]


def split_identifier(name: str) -> str:
    """Replicate split_identifier() from embedding.rs."""
    if "_" in name:
        parts = [p for p in name.split("_") if p]
        expanded = []
        for part in parts:
            s = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", part)
            s = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1 \2", s)
            expanded.extend(s.split())
        return " ".join(w.lower() for w in expanded if w)
    s = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", name)
    s = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1 \2", s)
    return " ".join(w.lower() for w in s.split() if w)


def build_runtime_positive(name: str, kind: str, signature: str, file_path: str) -> str:
    """Replicate build_embedding_text() from embedding.rs EXACTLY.

    Tests confirm:
      - "function hello in main.py: def hello():"
      - "class MyClass (My Class) in app.py: class MyClass:"
      - "variable CONFIG in config.py"  (no signature)
    """
    split = split_identifier(name)
    if split != name.lower():
        name_with_split = f"{name} ({split})"
    else:
        name_with_split = name

    file_ctx = f" in {file_path}" if file_path else ""

    if signature:
        return f"{kind} {name_with_split}{file_ctx}: {signature}"
    return f"{kind} {name_with_split}{file_ctx}"


def extract_short_name(func_name: str) -> str:
    """Extract the actual function/method name from fully qualified name.

    Examples:
      "ImageGraphCut.__msgc_step3_discontinuity_localization" → "__msgc_step3_discontinuity_localization"
      "MyClass.my_method" → "my_method"
      "simple_func" → "simple_func"
    """
    if "." in func_name:
        return func_name.split(".")[-1]
    return func_name


def extract_signature(func_code: str, language: str) -> str:
    """Extract function signature (first line of code).

    This matches what tree-sitter would extract as the signature.
    """
    first_line = func_code.split("\n")[0].strip()
    # Truncate overly long signatures
    return first_line[:150]


def is_quality_query(query: str) -> bool:
    """Filter out low-quality queries."""
    q = query.strip()
    if len(q) < MIN_QUERY_LEN:
        return False
    low = q.lower()
    if any(
        marker in low
        for marker in [
            "@inheritdoc",
            "@generated",
            "auto generated",
            "<!-- begin",
            "deepcopyinto",
            "todo:",
            "fixme:",
            "deprecated",
            "overrides",
            "@override",
            "deprecated:",
            "noinspection",
        ]
    ):
        return False
    # Skip queries that are just function signatures (not NL)
    if q.startswith("def ") or q.startswith("func ") or q.startswith("fn "):
        return False
    # Skip queries that are mostly code
    if q.count("{") > 2 or q.count(";") > 2:
        return False
    return True


def download_and_convert():
    from datasets import load_dataset

    all_pairs = []
    lang_counts = {}

    for lang in LANGUAGES:
        print(f"\n--- Downloading {lang} ---")
        count = 0
        skipped_query = 0
        skipped_name = 0
        skipped_path = 0

        try:
            ds = load_dataset(
                "code-search-net/code_search_net",
                lang,
                split="train",
            )
        except Exception as e:
            print(f"  ERROR loading {lang}: {e}")
            continue

        print(f"  Total rows: {len(ds)}")

        for row in ds:
            if count >= MAX_PER_LANG:
                break

            # Extract fields
            func_name = row.get("func_name", "")
            file_path = row.get("func_path_in_repository", "")
            doc = row.get("func_documentation_string", "")
            code = row.get("func_code_string", "")

            # Quality checks
            if not func_name or len(func_name) < 2:
                skipped_name += 1
                continue

            if not file_path:
                skipped_path += 1
                continue

            if not is_quality_query(doc):
                skipped_query += 1
                continue

            # Build EXACT runtime format
            short_name = extract_short_name(func_name)
            signature = extract_signature(code, lang)
            positive = build_runtime_positive(
                short_name, "function", signature, file_path
            )

            query = doc.strip()[:MAX_QUERY_LEN]

            all_pairs.append(
                {
                    "query": query,
                    "positive": positive,
                    "language": lang,
                }
            )
            count += 1

        lang_counts[lang] = count
        print(f"  Converted: {count}")
        print(
            f"  Skipped — query: {skipped_query}, name: {skipped_name}, path: {skipped_path}"
        )

    # Deduplicate
    seen = set()
    deduped = []
    for p in all_pairs:
        key = (p["query"][:50], p["positive"][:50])
        if key not in seen:
            seen.add(key)
            deduped.append(p)

    # Shuffle for in-batch negatives
    import random

    random.seed(42)
    random.shuffle(deduped)

    # Write output
    with OUTPUT.open("w") as f:
        for p in deduped:
            f.write(json.dumps(p, ensure_ascii=False) + "\n")

    # Stats
    from collections import Counter

    langs = Counter(p["language"] for p in deduped)
    print(f"\n=== RESULT: {len(deduped)} pairs ===")
    for lang, cnt in langs.most_common():
        print(f"  {lang}: {cnt}")
    print(f"\nWrote to {OUTPUT}")

    # Verify format matches runtime
    print("\n=== FORMAT VERIFICATION ===")
    sample = deduped[0]
    print(f"  Query:    {sample['query'][:80]}...")
    print(f"  Positive: {sample['positive'][:120]}...")
    print(f"  Expected: function {{name}} ({{split}}) in {{file_path}}: {{signature}}")


if __name__ == "__main__":
    download_and_convert()
