#!/usr/bin/env python3
"""Fail-closed contamination audit for embedding training manifests."""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path

from build_runtime_training_pipeline import normalize_query, parse_runtime_positive


SCRIPT_DIR = Path(__file__).parent
ROOT = SCRIPT_DIR.parent.parent
DEFAULT_PRODUCT = ROOT / "benchmarks" / "embedding-quality-dataset.json"
DEFAULT_EXTERNAL = ROOT / "benchmarks" / "external-retrieval-dataset.json"
DEFAULT_ROLE = ROOT / "benchmarks" / "role-retrieval-dataset.json"

QUERY_ARTIFACT_KEYS = (
    "train_path",
    "validation_path",
    "hard_negatives_path",
    "product_polish_path",
    "product_validation_path",
    "product_hard_negatives_path",
    "semantic_polish_path",
    "semantic_validation_path",
    "semantic_hard_negatives_path",
)


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--product-benchmark", default=str(DEFAULT_PRODUCT))
    parser.add_argument("--external-benchmark", default=str(DEFAULT_EXTERNAL))
    parser.add_argument("--role-benchmark", default=str(DEFAULT_ROLE))
    parser.add_argument("--output", default=str(SCRIPT_DIR / "contamination-audit-report.json"))
    return parser.parse_args()


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def iter_jsonl(path: Path):
    with path.open(encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            yield json.loads(line)


def benchmark_rows(path: Path, source: str) -> list[dict]:
    if not path.exists():
        return []
    raw = load_json(path)
    rows = []
    if isinstance(raw, list):
        items = raw
    elif isinstance(raw, dict) and isinstance(raw.get("rows"), list):
        items = raw["rows"]
    elif isinstance(raw, dict) and isinstance(raw.get("repos"), list):
        items = []
        for repo in raw["repos"]:
            repo_id = repo.get("repo_id") or repo.get("label") or "unknown"
            for item in repo.get("queries", []):
                item = dict(item)
                item["repo_id"] = repo_id
                items.append(item)
    else:
        items = []

    for item in items:
        query = item.get("query")
        if not query:
            continue
        rows.append(
            {
                "source": source,
                "query": query,
                "query_norm": normalize_query(query).lower(),
                "expected_symbol": item.get("expected_symbol"),
                "expected_file_suffix": item.get("expected_file_suffix"),
            }
        )
    return rows


def manifest_query_artifacts(manifest: dict) -> list[tuple[str, Path]]:
    artifacts = []
    for key in QUERY_ARTIFACT_KEYS:
        raw_path = manifest.get(key)
        if not raw_path:
            continue
        path = Path(raw_path)
        if not path.is_absolute():
            path = (ROOT / raw_path).resolve()
        artifacts.append((key, path))
    return artifacts


def parse_row_positive(row: dict):
    positive = row.get("positive")
    if not isinstance(positive, str):
        return None
    parsed = parse_runtime_positive(positive)
    if not parsed:
        return None
    return {
        "expected_symbol": parsed.get("name") or None,
        "expected_file_suffix": parsed.get("file") or None,
    }


def build_benchmark_indexes(rows: list[dict]):
    by_exact = defaultdict(list)
    by_norm = defaultdict(list)
    by_object = defaultdict(list)
    for row in rows:
        by_exact[row["query"]].append(row)
        by_norm[row["query_norm"]].append(row)
        if row.get("expected_symbol") and row.get("expected_file_suffix"):
            by_object[
                (
                    row["query_norm"],
                    row["expected_symbol"],
                    row["expected_file_suffix"],
                )
            ].append(row)
    return by_exact, by_norm, by_object


def audit_manifest(manifest_path: Path, benchmark_sets: list[tuple[str, Path]]) -> dict:
    manifest = load_json(manifest_path)
    benchmarks = []
    for source, path in benchmark_sets:
        benchmarks.extend(benchmark_rows(path, source))

    exact_index, norm_index, object_index = build_benchmark_indexes(benchmarks)

    overlap_counts = Counter()
    failures = []
    findings = []

    for artifact_key, artifact_path in manifest_query_artifacts(manifest):
        if not artifact_path.exists():
            failures.append(f"artifact missing: {artifact_key} -> {artifact_path}")
            continue
        for line_no, row in enumerate(iter_jsonl(artifact_path), start=1):
            query = row.get("query")
            if not isinstance(query, str) or not query.strip():
                continue
            query_norm = normalize_query(query).lower()
            parsed_positive = parse_row_positive(row)

            exact_matches = exact_index.get(query, [])
            norm_matches = norm_index.get(query_norm, [])
            copied_matches = []
            if parsed_positive:
                copied_matches = object_index.get(
                    (
                        query_norm,
                        parsed_positive["expected_symbol"],
                        parsed_positive["expected_file_suffix"],
                    ),
                    [],
                )

            row_findings = []
            if exact_matches:
                overlap_counts["exact_query_overlap"] += 1
                row_findings.append("exact_query_overlap")
            if norm_matches:
                overlap_counts["normalized_query_overlap"] += 1
                row_findings.append("normalized_query_overlap")
            if copied_matches:
                overlap_counts["copied_benchmark_object"] += 1
                row_findings.append("copied_benchmark_object")

            if row_findings:
                findings.append(
                    {
                        "artifact": artifact_key,
                        "path": str(artifact_path),
                        "line": line_no,
                        "query": query,
                        "query_norm": query_norm,
                        "flags": row_findings,
                        "benchmark_sources": sorted(
                            {
                                match["source"]
                                for match in exact_matches + norm_matches + copied_matches
                            }
                        ),
                        "parsed_positive": parsed_positive,
                    }
                )

    if overlap_counts["exact_query_overlap"]:
        failures.append(
            f"exact benchmark query text leaked into training artifacts ({overlap_counts['exact_query_overlap']} rows)"
        )
    if overlap_counts["normalized_query_overlap"]:
        failures.append(
            f"normalized benchmark query overlap detected ({overlap_counts['normalized_query_overlap']} rows)"
        )
    if overlap_counts["copied_benchmark_object"]:
        failures.append(
            f"copied benchmark row objects detected ({overlap_counts['copied_benchmark_object']} rows)"
        )

    return {
        "schema_version": "codelens-contamination-audit-v1",
        "manifest": str(manifest_path),
        "benchmarks": {
            source: str(path)
            for source, path in benchmark_sets
        },
        "artifact_count": len(manifest_query_artifacts(manifest)),
        "overlap_counts": dict(sorted(overlap_counts.items())),
        "findings": findings[:200],
        "finding_count": len(findings),
        "failures": failures,
        "passed": not failures,
    }


def main():
    args = parse_args()
    manifest_path = Path(args.manifest).expanduser().resolve()
    if not manifest_path.exists():
        raise SystemExit(f"manifest not found: {manifest_path}")

    benchmark_sets = [
        ("product", Path(args.product_benchmark).expanduser().resolve()),
        ("external", Path(args.external_benchmark).expanduser().resolve()),
        ("role", Path(args.role_benchmark).expanduser().resolve()),
    ]
    report = audit_manifest(manifest_path, benchmark_sets)
    output_path = Path(args.output).expanduser().resolve()
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(
        json.dumps(report, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(report, ensure_ascii=False, indent=2))
    if not report["passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
