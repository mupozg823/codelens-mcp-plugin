#!/usr/bin/env python3
"""Build a runtime-aligned training pipeline for generic CodeLens embedding tuning.

Outputs a manifest directory with:
  - train.jsonl: retrieval rows for Stage 2 (query, positive, negative?)
  - validation.jsonl: held-out retrieval pairs for evaluator / model selection
  - distill_texts.jsonl: generic text corpus for Stage 1 teacher alignment
  - hard_negatives.jsonl: mined hard-negative evidence for audit/debugging
  - manifest.json: paths + stats + holdout metadata

Design goals:
1. Match the current runtime embedding text format as closely as possible.
2. Keep benchmark holdout queries out of training artifacts.
3. Preserve generic performance by balancing sources and languages where possible.
4. Produce deterministic splits and reproducible manifests.
5. Generate multi-view positives and mined hard negatives without requiring a heavy model pass.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import random
import re
from collections import Counter, defaultdict
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
ROOT = SCRIPT_DIR.parent.parent

DEFAULT_OUTPUT_DIR = SCRIPT_DIR / "pipelines" / "runtime-generic-v1"
LEGACY_RUNTIME_INPUTS = [
    SCRIPT_DIR / "training_final_v4.jsonl",
    SCRIPT_DIR / "training_pairs_camelcase.jsonl",
    SCRIPT_DIR / "feedback_pairs_clean.jsonl",
]
DEFAULT_BENCH_HOLDOUT = ROOT / "benchmarks" / "embedding-quality-dataset.json"
TARGET_LANGUAGES = [
    "python",
    "javascript",
    "typescript",
    "java",
    "go",
    "ruby",
    "php",
    "rust",
]
MIN_GENERIC_LANG_PAIRS = 1000
DEFAULT_VIEW_PROFILES = (
    ("full", 1.0),
    ("no_path", 0.35),
    ("no_doc", 0.35),
    ("short", 0.15),
)
MAX_MINING_TOKENS = 8
MAX_CANDIDATES_PER_TOKEN = 256
MIN_NEGATIVE_SHARED_TOKENS = 2
NOISE_TOKENS = {
    "function",
    "class",
    "method",
    "module",
    "file",
    "return",
    "returns",
    "using",
    "value",
    "with",
    "from",
    "into",
    "this",
    "that",
    "used",
    "should",
}

MIN_QUERY_LEN = 15
MAX_QUERY_LEN = 300
MAX_SIGNATURE_LEN = 160
UNKNOWN_LANGS = {"", "unknown", "mixed"}

LANG_PATTERNS = {
    "python": r"def\s+([A-Za-z_][A-Za-z0-9_]*)",
    "javascript": r"(?:function\s+([A-Za-z_$][A-Za-z0-9_$]*)|(?:const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=)",
    "typescript": r"(?:function\s+([A-Za-z_$][A-Za-z0-9_$]*)|(?:const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=)",
    "java": r"(?:public|private|protected|static|final|\s)+(?:[\w<>\[\]?,.\s]+)\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(",
    "go": r"func\s+(?:\([^)]+\)\s+)?([A-Za-z_][A-Za-z0-9_]*)",
    "ruby": r"def\s+([A-Za-z_][A-Za-z0-9_?!]*)",
    "php": r"function\s+([A-Za-z_][A-Za-z0-9_]*)",
    "rust": r"fn\s+([A-Za-z_][A-Za-z0-9_]*)",
}

FILE_EXTENSION_LANG = {
    ".py": "python",
    ".js": "javascript",
    ".jsx": "javascript",
    ".ts": "typescript",
    ".tsx": "typescript",
    ".java": "java",
    ".go": "go",
    ".rb": "ruby",
    ".php": "php",
    ".rs": "rust",
}

RUNTIME_POSITIVE_RE = re.compile(
    r"^(?P<kind>[A-Za-z_]+)\s+"
    r"(?P<name>[A-Za-z_][A-Za-z0-9_]*)"
    r"(?:\s+\((?P<split>[^)]*)\))?"
    r"(?:\s+in\s+(?P<file>[^:]+?))?"
    r"(?::\s*(?P<signature>.*?))?"
    r"(?:\s+—\s+(?P<doc_hint>.*))?$"
)


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", default=str(DEFAULT_OUTPUT_DIR))
    parser.add_argument(
        "--codesearchnet-input", default=str(SCRIPT_DIR / "codesearchnet_pairs.jsonl")
    )
    parser.add_argument(
        "--codexglue-input",
        action="append",
        default=[],
        help="CodeXGLUE-style JSONL file with docstring/code/path/func_name rows. Can be passed multiple times.",
    )
    parser.add_argument(
        "--runtime-input",
        action="append",
        default=[],
        help="Runtime-format pair file. Can be passed multiple times.",
    )
    parser.add_argument(
        "--include-legacy-runtime-inputs",
        action="store_true",
        help="Include legacy local runtime inputs. Disabled by default to keep the pipeline clean-by-default.",
    )
    parser.add_argument("--holdout-benchmark", default=str(DEFAULT_BENCH_HOLDOUT))
    parser.add_argument("--max-csn-per-lang", type=int, default=8000)
    parser.add_argument("--validation-ratio", type=float, default=0.08)
    parser.add_argument("--distill-query-ratio", type=float, default=0.35)
    parser.add_argument("--distill-max-texts", type=int, default=30000)
    parser.add_argument(
        "--hard-negatives-per-query",
        type=int,
        default=1,
        help="Number of hard negatives to attach to each train row (0 disables hard-negative mining).",
    )
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument(
        "--no-multi-view",
        action="store_true",
        help="Skip multi-view expansion (use full view only). Useful for faster training runs.",
    )
    return parser.parse_args()


def iter_jsonl(path: Path):
    with path.open(encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            yield json.loads(line)


def normalize_space(text: str) -> str:
    return re.sub(r"\s+", " ", text).strip()


def normalize_query(query: str) -> str:
    return normalize_space(query)[:MAX_QUERY_LEN]


def first_line(text: str) -> str:
    for line in text.splitlines():
        line = normalize_space(line)
        if line:
            return line
    return ""


def split_identifier(name: str) -> str:
    if "_" in name:
        parts = [p for p in name.split("_") if p]
        expanded = []
        for part in parts:
            spaced = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", part)
            spaced = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1 \2", spaced)
            expanded.extend(spaced.split())
        return " ".join(word.lower() for word in expanded if word)
    spaced = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", name)
    spaced = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1 \2", spaced)
    return " ".join(word.lower() for word in spaced.split() if word)


def semantic_tokens(text: str) -> list[str]:
    tokens = re.findall(r"[A-Za-z][A-Za-z0-9_]+", normalize_space(text).lower())
    deduped = []
    seen = set()
    for token in tokens:
        token = token.strip("_")
        if len(token) < 3 or token in NOISE_TOKENS:
            continue
        if token not in seen:
            deduped.append(token)
            seen.add(token)
    return deduped


def normalize_func_name(name: str) -> str:
    name = normalize_space(name)
    if "." in name:
        return name.split(".")[-1]
    return name


def build_runtime_positive(
    name: str,
    kind: str,
    signature: str,
    file_path: str,
    doc_hint: str = "",
) -> str:
    split_name = split_identifier(name)
    name_with_split = f"{name} ({split_name})" if split_name != name.lower() else name
    file_ctx = f" in {file_path}" if file_path else ""
    signature = normalize_space(signature)[:MAX_SIGNATURE_LEN]
    base = f"{kind} {name_with_split}{file_ctx}"
    if signature:
        base = f"{base}: {signature}"
    doc_hint = normalize_space(doc_hint)
    if doc_hint:
        return f"{base} — {doc_hint[:60]}"
    return base


def is_quality_query(query: str) -> bool:
    q = normalize_query(query)
    if len(q) < MIN_QUERY_LEN:
        return False
    low = q.lower()
    if any(
        marker in low
        for marker in (
            "@inheritdoc",
            "@generated",
            "auto generated",
            "<!-- begin",
            "todo:",
            "fixme:",
            "deprecated",
            "@override",
        )
    ):
        return False
    if q.startswith(("def ", "func ", "fn ", "function ")):
        return False
    return True


def detect_query_type(query: str) -> str:
    q = normalize_query(query)
    words = q.split()
    if len(words) <= 3:
        return "short_phrase"
    if any(
        token in q.lower() for token in ("how ", "why ", "what ", "returns ", "return ")
    ):
        return "natural_language"
    return "mixed_natural_language"


def fingerprint(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def deterministic_float(seed: int, key: str) -> float:
    value = hashlib.sha256(f"{seed}:{key}".encode("utf-8")).hexdigest()[:16]
    return int(value, 16) / float(16**16)


def benchmark_holdout_queries(path: Path) -> set[str]:
    if not path.exists():
        return set()
    queries = set()
    text = path.read_text(encoding="utf-8")
    # Support both JSON array and JSONL formats
    try:
        items = json.loads(text)
        for item in items:
            if item.get("query"):
                queries.add(normalize_query(item["query"]).lower())
    except json.JSONDecodeError:
        for line in text.splitlines():
            line = line.strip()
            if not line:
                continue
            try:
                item = json.loads(line)
                if item.get("query"):
                    queries.add(normalize_query(item["query"]).lower())
            except json.JSONDecodeError:
                continue
    return queries


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


def parse_runtime_positive(text: str) -> dict | None:
    text = normalize_space(text)
    match = RUNTIME_POSITIVE_RE.match(text)
    if not match:
        return None
    data = match.groupdict()
    data["kind"] = (data.get("kind") or "function").strip().lower()
    data["name"] = (data.get("name") or "").strip()
    data["file"] = (data.get("file") or "").strip()
    data["signature"] = (data.get("signature") or "").strip()
    data["doc_hint"] = (data.get("doc_hint") or "").strip()
    return data if data["name"] else None


def infer_language_from_runtime_positive(parsed: dict) -> str:
    file_path = (parsed.get("file") or "").strip()
    suffix = Path(file_path).suffix.lower()
    if suffix in FILE_EXTENSION_LANG:
        return FILE_EXTENSION_LANG[suffix]

    signature = (parsed.get("signature") or "").strip()
    for language, pattern in LANG_PATTERNS.items():
        if re.search(pattern, signature):
            return language
    return "unknown"


def render_positive_view(metadata: dict, view: str) -> str:
    signature = metadata.get("signature", "")
    file_path = metadata.get("file", "")
    doc_hint = metadata.get("doc_hint", "")
    if view == "full":
        pass
    elif view == "no_path":
        file_path = ""
    elif view == "no_doc":
        doc_hint = ""
    elif view == "short":
        signature = ""
        file_path = ""
        doc_hint = ""
    else:
        raise ValueError(f"Unknown positive view: {view}")
    return build_runtime_positive(
        metadata["name"],
        metadata["kind"],
        signature,
        file_path,
        doc_hint,
    )


def normalize_runtime_positive(text: str) -> str:
    parsed = parse_runtime_positive(text)
    if not parsed:
        return normalize_space(text)
    return render_positive_view(parsed, "full")


def build_pair(
    query: str,
    positive: str,
    *,
    language: str,
    source: str,
    query_type: str | None = None,
) -> dict | None:
    query = normalize_query(query)
    positive = normalize_runtime_positive(positive)
    if not query or not positive or not is_quality_query(query):
        return None
    metadata = parse_runtime_positive(positive) or {}
    language = (language or "unknown").lower()
    if language in UNKNOWN_LANGS:
        inferred_language = infer_language_from_runtime_positive(metadata)
        if inferred_language not in UNKNOWN_LANGS:
            language = inferred_language
    return {
        "query": query,
        "positive": positive,
        "language": language or "unknown",
        "source": source,
        "query_type": query_type or detect_query_type(query),
        "has_real_path": bool(metadata.get("file")),
        "has_doc_hint": bool(metadata.get("doc_hint")),
        "view": "full",
        "base_id": fingerprint(f"{query}\n{positive}"),
        "id": fingerprint(f"{query}\n{positive}"),
    }


def load_codesearchnet_pairs(path: Path, max_per_lang: int) -> list[dict]:
    if not path.exists():
        return []
    counts: Counter[str] = Counter()
    pairs = []
    for obj in iter_jsonl(path):
        query = obj.get("query", "")
        code = obj.get("positive", "")
        language = (obj.get("language") or "unknown").lower()
        if counts[language] >= max_per_lang:
            continue
        name = extract_func_name(code, language)
        if not name:
            continue
        signature = first_line(code)
        doc_hint = first_line(query)
        positive = build_runtime_positive(
            name=name,
            kind="function",
            signature=signature,
            file_path="",
            doc_hint=doc_hint,
        )
        pair = build_pair(
            query,
            positive,
            language=language,
            source="codesearchnet_raw",
            query_type="natural_language",
        )
        if pair is None:
            continue
        counts[language] += 1
        pairs.append(pair)
    return pairs


def load_codexglue_pairs(path: Path, max_per_lang: int) -> list[dict]:
    if not path.exists():
        return []
    counts: Counter[str] = Counter()
    pairs = []
    for obj in iter_jsonl(path):
        query = obj.get("docstring") or obj.get("query") or ""
        code = (
            obj.get("code") or obj.get("original_string") or obj.get("positive") or ""
        )
        language = (obj.get("language") or "unknown").lower()
        if counts[language] >= max_per_lang:
            continue
        raw_name = obj.get("func_name") or extract_func_name(code, language) or ""
        name = normalize_func_name(raw_name)
        if not name:
            continue
        signature = first_line(code)
        doc_hint = first_line(query)
        file_path = normalize_space(obj.get("path") or "")
        positive = build_runtime_positive(
            name=name,
            kind="function",
            signature=signature,
            file_path=file_path,
            doc_hint=doc_hint,
        )
        pair = build_pair(
            query,
            positive,
            language=language,
            source="codexglue_raw",
            query_type="natural_language",
        )
        if pair is None:
            continue
        counts[language] += 1
        pairs.append(pair)
    return pairs


def load_runtime_pairs(path: Path) -> list[dict]:
    if not path.exists():
        return []
    pairs = []
    source_name = path.stem
    for obj in iter_jsonl(path):
        query = obj.get("query", "")
        positive = obj.get("positive", "")
        language = obj.get("language", "mixed")
        source = obj.get("source", source_name)
        pair = build_pair(query, positive, language=language, source=source)
        if pair is not None:
            pairs.append(pair)
    return pairs


def dedupe_pairs(pairs: list[dict]) -> list[dict]:
    seen = set()
    deduped = []
    for pair in pairs:
        key = (pair["query"].lower(), pair["positive"])
        if key in seen:
            continue
        seen.add(key)
        deduped.append(pair)
    return deduped


def filter_holdout_overlap(
    pairs: list[dict], holdout_queries: set[str]
) -> tuple[list[dict], int]:
    kept = []
    excluded = 0
    for pair in pairs:
        if pair["query"].lower() in holdout_queries:
            excluded += 1
            continue
        kept.append(pair)
    return kept, excluded


def split_pairs(
    pairs: list[dict], validation_ratio: float, seed: int
) -> tuple[list[dict], list[dict]]:
    buckets: dict[tuple[str, str, str], list[dict]] = defaultdict(list)
    for pair in pairs:
        bucket = (pair["source"], pair["language"], pair["query_type"])
        buckets[bucket].append(pair)

    train, validation = [], []
    for bucket_pairs in buckets.values():
        bucket_pairs.sort(key=lambda item: item["id"])
        if len(bucket_pairs) < 8:
            train.extend(bucket_pairs)
            continue
        val_target = max(1, round(len(bucket_pairs) * validation_ratio))
        val_count = 0
        bucket_train = []
        for pair in bucket_pairs:
            if (
                val_count < val_target
                and deterministic_float(seed, pair["id"]) < validation_ratio
            ):
                validation.append(pair)
                val_count += 1
            else:
                bucket_train.append(pair)
        while val_count < val_target and bucket_train:
            candidate = bucket_train.pop()
            validation.append(candidate)
            val_count += 1
        train.extend(bucket_train)
    train.sort(key=lambda item: item["id"])
    validation.sort(key=lambda item: item["id"])
    return train, validation


def expand_train_views(train_pairs: list[dict], seed: int) -> list[dict]:
    expanded = []
    for pair in train_pairs:
        metadata = parse_runtime_positive(pair["positive"])
        if not metadata:
            expanded.append(pair)
            continue

        seen_positive = set()
        for view_name, probability in DEFAULT_VIEW_PROFILES:
            if view_name != "full":
                key = f"{pair['base_id']}:{view_name}"
                if deterministic_float(seed, key) >= probability:
                    continue
            variant = dict(pair)
            variant["positive"] = render_positive_view(metadata, view_name)
            if variant["positive"] in seen_positive:
                continue
            seen_positive.add(variant["positive"])
            variant["view"] = view_name
            variant["has_real_path"] = view_name != "no_path" and bool(
                metadata.get("file")
            )
            variant["has_doc_hint"] = view_name not in {"no_doc", "short"} and bool(
                metadata.get("doc_hint")
            )
            variant["id"] = fingerprint(
                f"{pair['base_id']}\n{view_name}\n{variant['positive']}"
            )
            expanded.append(variant)

    expanded.sort(key=lambda item: item["id"])
    return expanded


def mining_tokens_for_pair(pair: dict) -> list[str]:
    parsed = parse_runtime_positive(pair["positive"]) or {}
    fields = [
        pair["query"],
        parsed.get("name", ""),
        split_identifier(parsed.get("name", "")),
        parsed.get("signature", ""),
        Path(parsed.get("file", "")).name,
        parsed.get("doc_hint", ""),
    ]
    tokens = []
    seen = set()
    for field in fields:
        for token in semantic_tokens(field):
            if token in seen:
                continue
            seen.add(token)
            tokens.append(token)
            if len(tokens) >= MAX_MINING_TOKENS:
                return tokens
    return tokens


def build_negative_index(
    rows: list[dict],
) -> tuple[dict[str, list[int]], dict[str, list[int]]]:
    token_index: dict[str, list[int]] = defaultdict(list)
    language_rows: dict[str, list[int]] = defaultdict(list)
    for idx, row in enumerate(rows):
        language_rows[row["language"]].append(idx)
        for token in mining_tokens_for_pair(row):
            bucket = token_index[f"{row['language']}::{token}"]
            if len(bucket) < MAX_CANDIDATES_PER_TOKEN:
                bucket.append(idx)
    return token_index, language_rows


def select_hard_negative(
    idx: int,
    rows: list[dict],
    token_index: dict[str, list[int]],
    language_rows: dict[str, list[int]],
    seed: int,
) -> dict | None:
    row = rows[idx]
    candidates: Counter[int] = Counter()
    for token in mining_tokens_for_pair(row):
        for candidate_idx in token_index.get(f"{row['language']}::{token}", []):
            if candidate_idx == idx:
                continue
            if rows[candidate_idx]["base_id"] == row["base_id"]:
                continue
            candidates[candidate_idx] += 1

    ranked = sorted(
        candidates.items(), key=lambda item: (-item[1], rows[item[0]]["id"])
    )
    for candidate_idx, score in ranked:
        if score < MIN_NEGATIVE_SHARED_TOKENS:
            break
        return rows[candidate_idx]

    pool = language_rows.get(row["language"], [])
    if len(pool) <= 1:
        return None
    offset = int(deterministic_float(seed, f"{row['id']}:fallback") * len(pool))
    for delta in range(len(pool)):
        candidate_idx = pool[(offset + delta) % len(pool)]
        if candidate_idx == idx:
            continue
        if rows[candidate_idx]["base_id"] == row["base_id"]:
            continue
        return rows[candidate_idx]
    return None


def attach_hard_negatives(
    train_rows: list[dict], negatives_per_query: int, seed: int
) -> tuple[list[dict], list[dict]]:
    if negatives_per_query <= 0:
        return train_rows, []

    token_index, language_rows = build_negative_index(train_rows)
    with_negatives = []
    negative_rows = []
    for idx, row in enumerate(train_rows):
        updated = dict(row)
        mined = []
        used_ids = {row["base_id"]}
        for negative_slot in range(negatives_per_query):
            negative = select_hard_negative(
                idx,
                train_rows,
                token_index,
                language_rows,
                seed + negative_slot,
            )
            if negative is None or negative["base_id"] in used_ids:
                break
            column = (
                "negative" if negative_slot == 0 else f"negative_{negative_slot + 1}"
            )
            updated[column] = negative["positive"]
            mined.append(
                {
                    "column": column,
                    "negative_id": negative["base_id"],
                    "negative": negative["positive"],
                }
            )
            used_ids.add(negative["base_id"])
        with_negatives.append(updated)
        if mined:
            negative_rows.append(
                {
                    "query": row["query"],
                    "positive": row["positive"],
                    "language": row["language"],
                    "source": row["source"],
                    "view": row.get("view", "full"),
                    "base_id": row["base_id"],
                    "negatives": mined,
                }
            )

    return with_negatives, negative_rows


def build_distill_texts(
    train_pairs: list[dict], query_ratio: float, max_texts: int
) -> list[dict]:
    positives = []
    queries = []
    seen_positive = set()
    seen_query = set()
    for pair in train_pairs:
        if pair["positive"] not in seen_positive:
            positives.append(
                {
                    "text": pair["positive"],
                    "role": "positive",
                    "language": pair["language"],
                    "source": pair["source"],
                }
            )
            seen_positive.add(pair["positive"])
        lowered_query = pair["query"].lower()
        if lowered_query not in seen_query:
            queries.append(
                {
                    "text": pair["query"],
                    "role": "query",
                    "language": pair["language"],
                    "source": pair["source"],
                }
            )
            seen_query.add(lowered_query)

    if max_texts <= 0:
        return []

    total_budget = min(max_texts, len(positives) + len(queries))
    query_budget = min(int(total_budget * query_ratio), len(queries))
    positive_budget = min(total_budget - query_budget, len(positives))

    # Backfill whichever side still has capacity so the corpus stays full.
    remaining = total_budget - (positive_budget + query_budget)
    if remaining > 0 and len(positives) > positive_budget:
        add = min(remaining, len(positives) - positive_budget)
        positive_budget += add
        remaining -= add
    if remaining > 0 and len(queries) > query_budget:
        query_budget += min(remaining, len(queries) - query_budget)

    selected_positives = positives[:positive_budget]
    selected_queries = queries[:query_budget]

    texts = []
    max_len = max(len(selected_positives), len(selected_queries))
    for idx in range(max_len):
        if idx < len(selected_positives):
            texts.append(selected_positives[idx])
        if idx < len(selected_queries):
            texts.append(selected_queries[idx])
    return texts


def write_jsonl(path: Path, rows: list[dict]) -> None:
    with path.open("w", encoding="utf-8") as f:
        for row in rows:
            f.write(json.dumps(row, ensure_ascii=False) + "\n")


def summarize_pairs(rows: list[dict]) -> dict:
    return {
        "count": len(rows),
        "languages": Counter(row["language"] for row in rows),
        "sources": Counter(row["source"] for row in rows),
        "query_types": Counter(row["query_type"] for row in rows),
        "views": Counter(row.get("view", "full") for row in rows),
        "with_real_path": sum(1 for row in rows if row["has_real_path"]),
        "with_doc_hint": sum(1 for row in rows if row["has_doc_hint"]),
        "with_hard_negative": sum(1 for row in rows if row.get("negative")),
    }


def counter_to_json(counter: Counter[str]) -> dict[str, int]:
    return dict(sorted(counter.items(), key=lambda item: (-item[1], item[0])))


def build_coverage_warnings(stats: dict) -> list[str]:
    warnings = []
    train = stats["train"]
    train_langs = train["languages"]
    train_count = max(train["count"], 1)

    for language in TARGET_LANGUAGES:
        count = train_langs.get(language, 0)
        if count < MIN_GENERIC_LANG_PAIRS:
            warnings.append(
                f"low {language} coverage: {count} train pairs (< {MIN_GENERIC_LANG_PAIRS})"
            )

    mixed_ratio = train_langs.get("mixed", 0) / train_count
    if mixed_ratio > 0.15:
        warnings.append(
            f"high mixed-language ratio: {mixed_ratio:.1%} of train pairs still unresolved"
        )

    path_ratio = train["with_real_path"] / train_count
    if path_ratio < 0.08:
        warnings.append(
            f"low real-path coverage: {path_ratio:.1%} of train positives include file paths"
        )

    hard_negative_ratio = train["with_hard_negative"] / train_count
    if hard_negative_ratio < 0.8:
        warnings.append(
            f"low hard-negative coverage: {hard_negative_ratio:.1%} of train rows received mined negatives"
        )

    return warnings


def main():
    args = parse_args()
    random.seed(args.seed)

    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    runtime_inputs = []
    if args.include_legacy_runtime_inputs:
        runtime_inputs.extend(LEGACY_RUNTIME_INPUTS)
    runtime_inputs.extend(Path(path) for path in args.runtime_input)
    codexglue_inputs = [Path(path) for path in args.codexglue_input]

    holdout_queries = benchmark_holdout_queries(Path(args.holdout_benchmark))

    all_pairs = []
    all_pairs.extend(
        load_codesearchnet_pairs(Path(args.codesearchnet_input), args.max_csn_per_lang)
    )
    for codexglue_input in codexglue_inputs:
        all_pairs.extend(load_codexglue_pairs(codexglue_input, args.max_csn_per_lang))
    for runtime_input in runtime_inputs:
        all_pairs.extend(load_runtime_pairs(runtime_input))

    all_pairs = dedupe_pairs(all_pairs)
    all_pairs, overlap_excluded = filter_holdout_overlap(all_pairs, holdout_queries)
    train_base_pairs, validation_pairs = split_pairs(
        all_pairs, args.validation_ratio, args.seed
    )
    if args.no_multi_view:
        train_pairs = list(train_base_pairs)
    else:
        train_pairs = expand_train_views(train_base_pairs, args.seed)
    train_pairs, hard_negative_rows = attach_hard_negatives(
        train_pairs,
        args.hard_negatives_per_query,
        args.seed,
    )
    distill_texts = build_distill_texts(
        train_pairs, args.distill_query_ratio, args.distill_max_texts
    )

    train_path = output_dir / "train.jsonl"
    validation_path = output_dir / "validation.jsonl"
    distill_path = output_dir / "distill_texts.jsonl"
    hard_negatives_path = output_dir / "hard_negatives.jsonl"
    stats_path = output_dir / "stats.json"
    manifest_path = output_dir / "manifest.json"

    write_jsonl(train_path, train_pairs)
    write_jsonl(validation_path, validation_pairs)
    write_jsonl(distill_path, distill_texts)
    write_jsonl(hard_negatives_path, hard_negative_rows)

    stats = {
        "total_pairs": len(all_pairs),
        "holdout_overlap_excluded": overlap_excluded,
        "train_base": summarize_pairs(train_base_pairs),
        "train": summarize_pairs(train_pairs),
        "validation": summarize_pairs(validation_pairs),
        "hard_negatives": {
            "rows": len(hard_negative_rows),
            "per_query": args.hard_negatives_per_query,
        },
        "distill_texts": {
            "count": len(distill_texts),
            "roles": counter_to_json(Counter(row["role"] for row in distill_texts)),
        },
    }
    stats["warnings"] = build_coverage_warnings(stats)
    stats_path.write_text(
        json.dumps(stats, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )

    manifest = {
        "schema_version": 1,
        "seed": args.seed,
        "train_path": str(train_path),
        "validation_path": str(validation_path),
        "distill_texts_path": str(distill_path),
        "hard_negatives_path": str(hard_negatives_path),
        "holdout_benchmark_path": str(Path(args.holdout_benchmark)),
        "stats_path": str(stats_path),
        "stats": {
            "total_pairs": stats["total_pairs"],
            "train_base_count": stats["train_base"]["count"],
            "train_count": stats["train"]["count"],
            "validation_count": stats["validation"]["count"],
            "distill_count": stats["distill_texts"]["count"],
            "hard_negative_rows": stats["hard_negatives"]["rows"],
            "holdout_overlap_excluded": overlap_excluded,
            "train_languages": counter_to_json(stats["train"]["languages"]),
            "validation_languages": counter_to_json(stats["validation"]["languages"]),
            "warnings": stats["warnings"],
        },
    }
    manifest_path.write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )

    print(f"Built runtime training pipeline at {output_dir}")
    print(json.dumps(manifest["stats"], indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
