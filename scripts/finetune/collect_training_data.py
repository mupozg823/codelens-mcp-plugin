#!/usr/bin/env python3
"""Collect training data for embedding fine-tuning from session telemetry and quality datasets.

Two data sources:
1. Quality dataset (benchmarks/embedding-quality-dataset.json) — curated query-symbol pairs
2. Session telemetry (benchmarks/results/*.json) — implicit feedback from tool call chains

Output: scripts/finetune/training_pairs.jsonl
Each line: {"query": "...", "positive": "...", "negative": "..."}
"""

import argparse
import json
import os
import random
import subprocess
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
ROOT = SCRIPT_DIR.parent.parent
QUALITY_DATASET = ROOT / "benchmarks" / "embedding-quality-dataset.json"
OUTPUT = SCRIPT_DIR / "training_pairs.jsonl"


def parse_args():
    parser = argparse.ArgumentParser(description="Collect fine-tuning training data")
    parser.add_argument(
        "--project", default=str(ROOT), help="Project path to extract symbols from"
    )
    parser.add_argument(
        "--binary",
        default=os.environ.get(
            "CODELENS_BIN", str(ROOT / "target" / "release" / "codelens-mcp")
        ),
    )
    parser.add_argument("--output", default=str(OUTPUT))
    parser.add_argument(
        "--negatives-per-positive",
        type=int,
        default=5,
        help="Hard negatives per positive pair",
    )
    return parser.parse_args()


def run_tool(binary, project, cmd, args, timeout=30):
    """Call CodeLens binary and return parsed JSON response."""
    argv = [
        binary,
        project,
        "--preset",
        "full",
        "--cmd",
        cmd,
        "--args",
        json.dumps(args),
    ]
    result = subprocess.run(argv, capture_output=True, text=True, timeout=timeout)
    if result.returncode != 0:
        return None
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError:
        return None


def get_all_symbols(binary, project):
    """Get all indexed symbols for negative sampling."""
    symbols = []
    # Use multiple fuzzy searches to build a broad symbol pool
    for seed in ["a", "e", "i", "o", "s", "t", "g", "r", "c", "f"]:
        resp3 = run_tool(
            binary,
            project,
            "search_symbols_fuzzy",
            {"query": seed, "max_results": 30, "fuzzy_threshold": 0.0},
        )
        if resp3 and resp3.get("success") and resp3.get("data"):
            data = resp3["data"]
            if isinstance(data, dict) and "results" in data:
                for sym in data["results"]:
                    text = build_symbol_text(sym)
                    if text and text not in symbols:
                        symbols.append(text)

    return symbols


def build_symbol_text(sym):
    """Build embedding text from a symbol record (mirrors Rust build_embedding_text)."""
    name = sym.get("name", "")
    kind = sym.get("kind", "")
    file_path = sym.get("file_path", sym.get("file", ""))
    signature = sym.get("signature", "")

    if not name:
        return None

    file_ctx = f" in {file_path}" if file_path else ""
    if signature:
        return f"{kind} {name}{file_ctx}: {signature}"
    return f"{kind} {name}{file_ctx}"


def pairs_from_quality_dataset(binary, project, negatives_per_positive):
    """Generate training pairs from the curated quality dataset."""
    if not QUALITY_DATASET.exists():
        print(f"Quality dataset not found: {QUALITY_DATASET}")
        return []

    with open(QUALITY_DATASET) as f:
        queries = json.load(f)

    print(f"Loading {len(queries)} queries from quality dataset...")

    # Get symbol pool for negatives
    all_symbols = get_all_symbols(binary, project)
    if not all_symbols:
        print("Warning: could not load symbol pool, skipping negative sampling")

    pairs = []
    for entry in queries:
        query = entry["query"]
        expected_symbol = entry["expected_symbol"]
        expected_file = entry.get("expected_file_suffix", "")

        # Find the positive symbol
        resp = run_tool(
            binary,
            project,
            "find_symbol",
            {"name": expected_symbol, "include_body": False},
        )
        if not resp or not resp.get("success"):
            continue

        data = resp.get("data", {})
        results = data.get("results", data.get("symbols", []))
        if not results:
            continue

        # Pick the result matching the expected file
        positive_sym = None
        for sym in results:
            fp = sym.get("file_path", sym.get("file", ""))
            if expected_file and fp.endswith(expected_file):
                positive_sym = sym
                break
        if not positive_sym:
            positive_sym = results[0]

        positive_text = build_symbol_text(positive_sym)
        if not positive_text:
            continue

        # Sample hard negatives (random symbols that are NOT the positive)
        if all_symbols:
            candidates = [s for s in all_symbols if s != positive_text]
            negatives = random.sample(
                candidates, min(negatives_per_positive, len(candidates))
            )
        else:
            negatives = []

        for neg in negatives:
            pairs.append({"query": query, "positive": positive_text, "negative": neg})

        # Also add a pair without negative (for contrastive loss with in-batch negatives)
        if not negatives:
            pairs.append({"query": query, "positive": positive_text, "negative": ""})

    return pairs


def main():
    args = parse_args()
    print(f"Project: {args.project}")
    print(f"Binary: {args.binary}")

    all_pairs = []

    # Source 1: Quality dataset
    quality_pairs = pairs_from_quality_dataset(
        args.binary, args.project, args.negatives_per_positive
    )
    all_pairs.extend(quality_pairs)
    print(f"Quality dataset: {len(quality_pairs)} triplets")

    # Source 2: Session telemetry (future — implicit feedback from tool chains)
    # TODO: Parse session exports to find semantic_search→find_symbol chains
    # and extract (query, selected_symbol) pairs

    if not all_pairs:
        print("No training pairs collected. Ensure binary is built and dataset exists.")
        sys.exit(1)

    # Write output
    output_path = Path(args.output)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    with open(output_path, "w") as f:
        for pair in all_pairs:
            f.write(json.dumps(pair, ensure_ascii=False) + "\n")

    print(f"\nWrote {len(all_pairs)} training pairs to {output_path}")
    print(f"  Unique queries: {len(set(p['query'] for p in all_pairs))}")
    print(f"  With negatives: {sum(1 for p in all_pairs if p['negative'])}")


if __name__ == "__main__":
    main()
