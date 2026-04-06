#!/usr/bin/env python3
"""Analyze semantic search misses and suggest training data additions."""
import json, sys
from pathlib import Path

results_file = sys.argv[1] if len(sys.argv) > 1 else "benchmarks/embedding-quality-results-v2-final.json"
with open(results_file) as f:
    data = json.load(f)

semantic = [m for m in data["methods"] if m["method"] == "semantic_search"][0]
misses = [r for r in semantic["rows"] if r["rank"] is None]
hits = [r for r in semantic["rows"] if r["rank"] is not None]

print(f"=== Semantic Search Miss Analysis ===")
print(f"Total: {len(semantic['rows'])}, Hits: {len(hits)}, Misses: {len(misses)}")
print(f"MRR: {semantic['mrr']:.3f}")
print()

# Generate training pairs for misses
training_additions = []
for r in misses:
    query = r["query"]
    expected = r["expected_symbol"]
    expected_file = r.get("expected_file_suffix", "")
    top = r.get("top_candidate", {})
    top_name = top.get("name", "?")
    top_file = top.get("file", "?")

    print(f"MISS: \"{query[:60]}\"")
    print(f"  expected: {expected} in *{expected_file}")
    print(f"  got:      {top_name} in {top_file[:50]}")
    print(f"  → near-miss" if expected.lower() in top_name.lower() or top_name.lower() in expected.lower() else f"  → wrong target")
    print()

    # Build CodeLens-format positive for training
    positive = f"function {expected} in {expected_file}: {expected}"
    training_additions.append({
        "query": query,
        "positive": positive,
        "negative": "",
        "source": "benchmark_miss_feedback",
    })

# Write training additions
out = Path("scripts/finetune/training_pairs_miss_feedback.jsonl")
with out.open("w") as f:
    for pair in training_additions:
        f.write(json.dumps(pair, ensure_ascii=False) + "\n")

print(f"\nWrote {len(training_additions)} feedback pairs to {out}")
print("Add these to next training run for targeted MRR improvement.")
