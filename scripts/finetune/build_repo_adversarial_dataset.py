#!/usr/bin/env python3
"""Build repo-local adversarial retrieval rows without reusing benchmark queries."""

from __future__ import annotations

import argparse
import json
import subprocess
from collections import Counter, defaultdict
from pathlib import Path

from build_runtime_training_pipeline import (
    FILE_EXTENSION_LANG,
    build_runtime_positive,
    fingerprint,
    normalize_query,
    split_identifier,
)


SCRIPT_DIR = Path(__file__).parent
ROOT = SCRIPT_DIR.parent.parent
DEFAULT_BINARY = ROOT / "target" / "release" / "codelens-mcp"
DEFAULT_BENCHMARK = ROOT / "benchmarks" / "embedding-quality-dataset.json"
DEFAULT_GATE_REPORT = SCRIPT_DIR / "gate-results" / "promotion-gate" / "promotion-gate-report.json"
DEFAULT_OUTPUT = SCRIPT_DIR / "repo_local_adversarial.jsonl"
DEFAULT_EVAL_OUTPUT = ROOT / "benchmarks" / "role-retrieval-dataset.json"
DEFAULT_HOLDOUT_MODULO = 5

ROLE_STOPWORDS = {
    "tool",
    "handler",
    "function",
    "entrypoint",
    "helper",
    "query",
    "symbol",
    "code",
}


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--project", default=str(ROOT))
    parser.add_argument("--binary", default=str(DEFAULT_BINARY))
    parser.add_argument("--benchmark-dataset", default=str(DEFAULT_BENCHMARK))
    parser.add_argument("--gate-report", default=str(DEFAULT_GATE_REPORT))
    parser.add_argument("--output", default=str(DEFAULT_OUTPUT))
    parser.add_argument("--eval-output", default=str(DEFAULT_EVAL_OUTPUT))
    parser.add_argument("--max-negatives", type=int, default=3)
    parser.add_argument("--holdout-modulo", type=int, default=DEFAULT_HOLDOUT_MODULO)
    return parser.parse_args()


def run_tool(binary: str, project: str, cmd: str, arguments: dict) -> dict:
    result = subprocess.run(
        [
            binary,
            project,
            "--preset",
            "balanced",
            "--cmd",
            cmd,
            "--args",
            json.dumps(arguments),
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0 or not result.stdout.strip():
        raise RuntimeError(f"{cmd} failed: {result.stderr.strip()}")
    payload = json.loads(result.stdout.splitlines()[-1])
    if not payload.get("success") or "data" not in payload:
        raise RuntimeError(f"{cmd} failed: {json.dumps(payload, ensure_ascii=False)}")
    return payload["data"]


def find_symbol(binary: str, project: str, name: str, file_suffix: str = "") -> dict | None:
    try:
        data = run_tool(binary, project, "find_symbol", {"name": name, "include_body": False})
    except RuntimeError:
        return None
    symbols = data.get("symbols", [])
    if file_suffix:
        symbols = [
            symbol
            for symbol in symbols
            if str(symbol.get("file_path", "")).endswith(file_suffix)
        ]
    return symbols[0] if symbols else None


def get_symbols_overview(binary: str, project: str, path: str) -> list[dict]:
    try:
        data = run_tool(binary, project, "get_symbols_overview", {"path": path, "depth": 1})
    except RuntimeError:
        return []
    return list(data.get("symbols", []))


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def symbol_language(symbol: dict) -> str:
    suffix = Path(symbol["file_path"]).suffix.lower()
    return FILE_EXTENSION_LANG.get(suffix, "unknown")


def symbol_role(name: str) -> str:
    lowered = name.lower()
    if lowered.startswith(("is_", "has_", "can_", "should_", "prefers_")):
        return "predicate"
    if lowered.endswith("_handler") or lowered.startswith(
        ("dispatch_", "run_", "index_", "rename_", "move_", "change_", "inline_", "refactor_")
    ):
        return "entrypoint"
    if lowered.startswith(("build_", "make_", "create_", "register_", "upsert_")):
        return "builder"
    if lowered.startswith(("find_", "collect_", "resolve_", "inspect_", "count_", "parse_", "extract_")):
        return "helper"
    return "generic"


def content_tokens(name: str) -> list[str]:
    return [
        token
        for token in split_identifier(name).split()
        if token and token not in ROLE_STOPWORDS
    ]


def core_phrase(name: str) -> str:
    tokens = content_tokens(name)
    if tokens:
        return " ".join(tokens)
    return split_identifier(name) or name


def query_variants(symbol: dict) -> list[tuple[str, str]]:
    phrase = core_phrase(symbol["name"])
    role = symbol_role(symbol["name"])
    if role == "entrypoint":
        variants = [
            (f"{phrase} entrypoint", "short_phrase"),
            (f"where is the {phrase} entrypoint", "natural_language"),
        ]
    elif role == "predicate":
        variants = [
            (f"{phrase} predicate", "short_phrase"),
            (f"check whether {phrase}", "natural_language"),
        ]
    elif role == "builder":
        variants = [
            (f"{phrase} builder", "short_phrase"),
            (f"where is {phrase} built", "natural_language"),
        ]
    elif role == "helper":
        variants = [
            (f"{phrase} helper", "short_phrase"),
            (f"where is {phrase} implemented", "natural_language"),
        ]
    else:
        variants = [
            (phrase, "short_phrase"),
            (f"where is {phrase} implemented", "natural_language"),
        ]
    seen = set()
    deduped = []
    for query, query_type in variants:
        query = normalize_query(query)
        if query and query not in seen:
            seen.add(query)
            deduped.append((query, query_type))
    return deduped


def eval_query_variants(symbol: dict) -> list[tuple[str, str]]:
    phrase = core_phrase(symbol["name"])
    role = symbol_role(symbol["name"])
    if role == "entrypoint":
        variants = [
            (f"primary {phrase} handler", "short_phrase"),
            (f"which entrypoint handles {phrase}", "natural_language"),
        ]
    elif role == "predicate":
        variants = [
            (f"{phrase} decision check", "short_phrase"),
            (f"which predicate decides {phrase}", "natural_language"),
        ]
    elif role == "builder":
        variants = [
            (f"{phrase} construction", "short_phrase"),
            (f"which builder creates {phrase}", "natural_language"),
        ]
    elif role == "helper":
        variants = [
            (f"{phrase} internal helper", "short_phrase"),
            (f"which helper implements {phrase}", "natural_language"),
        ]
    else:
        variants = [
            (f"{phrase} primary implementation", "short_phrase"),
            (f"which symbol is responsible for {phrase}", "natural_language"),
        ]
    seen = set()
    deduped = []
    for query, query_type in variants:
        query = normalize_query(query)
        if query and query not in seen:
            seen.add(query)
            deduped.append((query, query_type))
    return deduped


def runtime_positive(symbol: dict) -> str:
    return build_runtime_positive(
        name=symbol["name"],
        kind=symbol["kind"],
        signature=symbol.get("signature", ""),
        file_path=symbol["file_path"],
        doc_hint="",
    )


def load_gate_candidates(path: Path) -> dict[str, set[str]]:
    if not path.exists():
        return {}
    report = load_json(path)
    query_to_names: dict[str, set[str]] = defaultdict(set)
    for section in ("baseline", "candidate"):
        methods = report.get(section, {})
        for method_name in ("semantic_search", "get_ranked_context"):
            for row in methods.get(method_name, {}).get("rows", []):
                top = row.get("top_candidate") or {}
                name = top.get("name")
                if name and name != row.get("expected_symbol"):
                    query_to_names[row["query"]].add(name)
    return query_to_names


def holdout_group(symbol: dict) -> str:
    parent = str(Path(symbol["file_path"]).parent)
    return fingerprint(parent)[:12]


def is_eval_holdout(symbol: dict, modulo: int) -> bool:
    if modulo <= 1:
        return True
    bucket = int(holdout_group(symbol), 16)
    return bucket % modulo == 0


def score_candidate(positive: dict, candidate: dict, failed_names: set[str]) -> int:
    score = 0
    if candidate["name"] in failed_names:
        score += 8
    if candidate["file_path"] == positive["file_path"]:
        score += 6
    shared = set(content_tokens(positive["name"])) & set(content_tokens(candidate["name"]))
    score += 4 * len(shared)
    if symbol_role(positive["name"]) != symbol_role(candidate["name"]):
        score += 2
    if candidate["kind"] == positive["kind"]:
        score += 1
    if Path(candidate["file_path"]).parent == Path(positive["file_path"]).parent:
        score += 1
    return score


def resolve_named_candidates(
    binary: str,
    project: str,
    names: set[str],
    positive: dict,
) -> list[dict]:
    resolved = []
    for name in sorted(names):
        symbol = find_symbol(binary, project, name)
        if symbol and symbol["id"] != positive["id"]:
            resolved.append(symbol)
    return resolved


def choose_negatives(
    binary: str,
    project: str,
    positive: dict,
    failed_names: set[str],
    max_negatives: int,
) -> list[dict]:
    candidates: dict[str, dict] = {}
    for symbol in get_symbols_overview(binary, project, positive["file_path"]):
        if symbol["id"] != positive["id"]:
            candidates[symbol["id"]] = symbol
    for symbol in resolve_named_candidates(binary, project, failed_names, positive):
        candidates[symbol["id"]] = symbol

    ranked = sorted(
        candidates.values(),
        key=lambda candidate: (
            -score_candidate(positive, candidate, failed_names),
            candidate["file_path"],
            candidate["line"],
            candidate["name"],
        ),
    )
    negatives = []
    seen_positive = set()
    for candidate in ranked:
        positive_text = runtime_positive(candidate)
        if positive_text in seen_positive:
            continue
        if score_candidate(positive, candidate, failed_names) <= 0:
            continue
        negatives.append(candidate)
        seen_positive.add(positive_text)
        if len(negatives) >= max_negatives:
            break
    return negatives


def build_rows(
    args,
    benchmark_rows: list[dict],
    gate_candidates: dict[str, set[str]],
) -> tuple[list[dict], list[dict]]:
    train_rows = []
    eval_rows = []
    for item in benchmark_rows:
        if item.get("query_type") == "identifier":
            continue
        positive = find_symbol(
            args.binary,
            args.project,
            item["expected_symbol"],
            item.get("expected_file_suffix", ""),
        )
        if not positive:
            continue
        negatives = choose_negatives(
            args.binary,
            args.project,
            positive,
            gate_candidates.get(item["query"], set()),
            args.max_negatives,
        )
        if not negatives:
            continue
        positive_text = runtime_positive(positive)
        adversarial_group = fingerprint(f"{positive['file_path']}::{positive['name']}")[:12]
        role = symbol_role(positive["name"])
        if is_eval_holdout(positive, args.holdout_modulo):
            for query, query_type in eval_query_variants(positive):
                eval_rows.append(
                    {
                        "query": query,
                        "query_type": query_type,
                        "expected_symbol": positive["name"],
                        "expected_file_suffix": positive["file_path"],
                        "language": symbol_language(positive),
                        "role": role,
                        "adversarial_group": adversarial_group,
                        "negative_symbols": [negative["name"] for negative in negatives],
                        "negative_file_suffixes": [
                            negative["file_path"] for negative in negatives
                        ],
                    }
                )
            continue

        for query, query_type in query_variants(positive):
            row = {
                "query": query,
                "query_type": query_type,
                "positive": positive_text,
                "language": symbol_language(positive),
                "source": "repo_local_adversarial",
                "product_focus": True,
                "semantic_focus": True,
                "adversarial_focus": True,
                "role_focus": role,
                "adversarial_group": adversarial_group,
            }
            for index, negative in enumerate(negatives, start=1):
                column = "negative" if index == 1 else f"negative_{index}"
                row[column] = runtime_positive(negative)
            train_rows.append(row)

    def dedupe_rows(rows: list[dict], key_fields: tuple[str, ...]) -> list[dict]:
        deduped = []
        seen = set()
        for row in rows:
            key = tuple(row[field] for field in key_fields)
            if key in seen:
                continue
            seen.add(key)
            deduped.append(row)
        return deduped

    return (
        dedupe_rows(train_rows, ("query", "positive")),
        dedupe_rows(eval_rows, ("query", "expected_symbol", "expected_file_suffix")),
    )


def main():
    args = parse_args()
    benchmark_rows = load_json(Path(args.benchmark_dataset))
    gate_candidates = load_gate_candidates(Path(args.gate_report))
    train_rows, eval_rows = build_rows(args, benchmark_rows, gate_candidates)
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("w", encoding="utf-8") as handle:
        for row in train_rows:
            handle.write(json.dumps(row, ensure_ascii=False) + "\n")
    eval_output = Path(args.eval_output)
    eval_output.parent.mkdir(parents=True, exist_ok=True)
    eval_output.write_text(
        json.dumps(
            {
                "schema_version": "codelens-role-retrieval-dataset-v1",
                "project": str(Path(args.project).resolve()),
                "rows": eval_rows,
            },
            ensure_ascii=False,
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    print(
        json.dumps(
            {
                "output": str(output),
                "train_count": len(train_rows),
                "eval_output": str(eval_output),
                "eval_count": len(eval_rows),
                "train_query_types": dict(
                    sorted(Counter(row["query_type"] for row in train_rows).items())
                ),
                "eval_query_types": dict(
                    sorted(Counter(row["query_type"] for row in eval_rows).items())
                ),
                "train_languages": dict(
                    sorted(Counter(row["language"] for row in train_rows).items())
                ),
                "eval_languages": dict(
                    sorted(Counter(row["language"] for row in eval_rows).items())
                ),
            },
            ensure_ascii=False,
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
