#!/usr/bin/env python3
"""Build high-quality training dataset aligned with CodeLens runtime embedding format.

Key principles (from SPENCER/LoRACode analysis + failed experiments):
1. positive MUST match build_embedding_text() runtime format exactly
2. query must be meaningful NL (>10 chars, no auto-generated junk)
3. No fake paths — use actual filenames or omit path entirely
4. MNRL needs diverse in-batch negatives → shuffle well
5. Quality > quantity — 10K clean pairs > 100K noisy pairs

Runtime embedding format (from embedding.rs build_embedding_text):
  "{kind} {name} ({split_name}) in {file}: {signature}"
  With docstring: "{base} — {first_line_of_docstring}"
"""

import argparse
import json
import re
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent

MIN_QUERY_LEN = 15  # Skip very short/meaningless queries
MAX_QUERY_LEN = 300  # Truncate overly long queries


def split_identifier(name: str) -> str:
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
    """Replicate build_embedding_text() from embedding.rs."""
    split = split_identifier(name)
    name_with_split = f"{name} ({split})" if split != name.lower() else name
    file_ctx = f" in {file_path}" if file_path else ""
    if signature:
        return f"{kind} {name_with_split}{file_ctx}: {signature}"
    return f"{kind} {name_with_split}{file_ctx}"


def is_quality_query(query: str) -> bool:
    """Filter out low-quality queries."""
    q = query.strip()
    if len(q) < MIN_QUERY_LEN:
        return False
    # Skip auto-generated junk
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
        ]
    ):
        return False
    # Skip queries that are just function signatures (not NL)
    if q.startswith("def ") or q.startswith("func ") or q.startswith("fn "):
        return False
    return True


def process_codesearchnet_raw(max_per_lang: int = 10000) -> list[dict]:
    """Load CSN raw pairs — real code as positive, real docstrings as query."""
    path = SCRIPT_DIR / "codesearchnet_pairs.jsonl"
    if not path.exists():
        print(f"  SKIP: {path} not found")
        return []

    pairs = []
    lang_counts: dict[str, int] = {}

    with path.open() as f:
        for line in f:
            obj = json.loads(line)
            query = obj.get("query", "").strip()
            code = obj.get("positive", "").strip()
            lang = obj.get("language", "unknown")

            if not is_quality_query(query):
                continue
            if lang_counts.get(lang, 0) >= max_per_lang:
                continue

            # Extract function name from code
            name = extract_func_name(code, lang)
            if not name or len(name) < 2:
                continue

            # Build runtime-format positive (no fake path)
            sig = code.split("\n")[0].strip()[:120]
            positive = build_runtime_positive(name, "function", sig, "")

            query = query[:MAX_QUERY_LEN]

            pairs.append(
                {
                    "query": query,
                    "positive": positive,
                    "language": lang,
                }
            )
            lang_counts[lang] = lang_counts.get(lang, 0) + 1

    print(f"  CSN raw: {len(pairs)} quality pairs")
    for lang, cnt in sorted(lang_counts.items()):
        print(f"    {lang}: {cnt}")
    return pairs


def extract_func_name(code: str, lang: str) -> str | None:
    patterns = {
        "python": r"def\s+(\w+)",
        "javascript": r"(?:function\s+(\w+)|(?:const|let|var)\s+(\w+)\s*=)",
        "java": r"(?:public|private|protected|static|final|\s)+(?:[\w<>\[\]?,.\s]+)\s+(\w+)\s*\(",
        "go": r"func\s+(?:\([^)]+\)\s+)?(\w+)",
        "ruby": r"def\s+([\w?!]+)",
        "php": r"function\s+(\w+)",
    }
    pattern = patterns.get(lang)
    if not pattern:
        return None
    m = re.search(pattern, code)
    if not m:
        return None
    return m.group(1) or (m.group(2) if m.lastindex and m.lastindex >= 2 else None)


def process_local_projects() -> list[dict]:
    """Load local project data with real file paths."""
    path = SCRIPT_DIR / "training_pairs_camelcase.jsonl"
    if not path.exists():
        return []

    pairs = []
    with path.open() as f:
        for line in f:
            obj = json.loads(line)
            query = obj.get("query", "").strip()
            positive = obj.get("positive", "").strip()

            if not is_quality_query(query):
                continue
            if not positive:
                continue

            pairs.append(
                {
                    "query": query[:MAX_QUERY_LEN],
                    "positive": positive,
                    "language": "mixed",
                }
            )

    print(f"  Local projects: {len(pairs)} quality pairs")
    return pairs


def process_feedback() -> list[dict]:
    """Load benchmark miss feedback pairs."""
    path = SCRIPT_DIR / "training_pairs_miss_feedback.jsonl"
    if not path.exists():
        return []

    pairs = []
    with path.open() as f:
        for line in f:
            obj = json.loads(line)
            pairs.append(
                {
                    "query": obj["query"],
                    "positive": obj["positive"],
                    "language": "rust",
                }
            )
    print(f"  Feedback: {len(pairs)} pairs")
    return pairs


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--max-per-lang", type=int, default=8000)
    parser.add_argument(
        "--output", default=str(SCRIPT_DIR / "training_quality_v1.jsonl")
    )
    args = parser.parse_args()

    print("=== Building quality dataset ===")
    all_pairs = []

    all_pairs.extend(process_codesearchnet_raw(args.max_per_lang))
    all_pairs.extend(process_local_projects())
    all_pairs.extend(process_feedback())

    # Deduplicate
    seen = set()
    deduped = []
    for p in all_pairs:
        key = (p["query"][:50], p["positive"][:50])
        if key in seen:
            continue
        seen.add(key)
        deduped.append(p)

    # Shuffle for good in-batch negatives
    import random

    random.seed(42)
    random.shuffle(deduped)

    output = Path(args.output)
    with output.open("w") as f:
        for p in deduped:
            f.write(json.dumps(p, ensure_ascii=False) + "\n")

    # Stats
    from collections import Counter

    langs = Counter(p["language"] for p in deduped)
    print(f"\n=== Quality dataset: {len(deduped)} pairs ===")
    for lang, cnt in langs.most_common():
        print(f"  {lang}: {cnt}")
    print(f"Wrote to {output}")


if __name__ == "__main__":
    main()
