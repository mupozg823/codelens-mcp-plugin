#!/usr/bin/env python3
"""Compare base all-MiniLM-L12-v2 vs V6 on NL benchmark queries.

Tests whether V6 full fine-tuning damaged the base model's NL capabilities.
"""

import json
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
ROOT = SCRIPT_DIR.parent.parent
BENCHMARK = ROOT / "benchmarks" / "embedding-quality-dataset.json"
V6_MODEL = SCRIPT_DIR / "output" / "v6-internet" / "model"


def load_benchmark():
    with open(BENCHMARK) as f:
        dataset = json.load(f)
    # Filter NL queries only
    nl_queries = [q for q in dataset if q["query_type"] == "natural_language"]
    short_queries = [q for q in dataset if q["query_type"] == "short_phrase"]
    id_queries = [q for q in dataset if q["query_type"] == "identifier"]
    return nl_queries, short_queries, id_queries


def build_runtime_positive(name: str, kind: str, file_path: str) -> str:
    """Replicate build_embedding_text from embedding.rs."""
    import re

    # split_identifier
    if "_" in name:
        parts = [p for p in name.split("_") if p]
        expanded = []
        for part in parts:
            spaced = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", part)
            spaced = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1 \2", spaced)
            expanded.extend(spaced.split())
        split = " ".join(w.lower() for w in expanded if w)
    else:
        spaced = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", name)
        spaced = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1 \2", spaced)
        split = " ".join(w.lower() for w in spaced.split() if w)

    name_with_split = f"{name} ({split})" if split != name.lower() else name
    file_ctx = f" in {file_path}" if file_path else ""
    return f"{kind} {name_with_split}{file_ctx}"


def evaluate_model(model, queries, label):
    """Compute MRR and mean cosine sim for a set of queries."""
    import torch

    # Build query texts and positive texts
    query_texts = [q["query"] for q in queries]
    positive_texts = [
        build_runtime_positive(
            q["expected_symbol"], "function", q["expected_file_suffix"]
        )
        for q in queries
    ]

    if not query_texts:
        return {}

    with torch.no_grad():
        q_embs = model.encode(
            query_texts, batch_size=32, convert_to_tensor=True, show_progress_bar=False
        )
        p_embs = model.encode(
            positive_texts,
            batch_size=32,
            convert_to_tensor=True,
            show_progress_bar=False,
        )

    # Normalize
    q_norm = torch.nn.functional.normalize(q_embs, p=2, dim=1)
    p_norm = torch.nn.functional.normalize(p_embs, p=2, dim=1)

    # Cosine similarity matrix (queries x positives)
    sim_matrix = q_norm @ p_norm.T

    # MRR: for each query, find rank of its correct positive
    ranks = []
    similarities = []
    for i in range(len(query_texts)):
        scores = sim_matrix[i]
        correct_score = scores[i].item()
        similarities.append(correct_score)
        # Rank = number of items with higher score + 1
        rank = (scores > correct_score).sum().item() + 1
        ranks.append(rank)

    mrr = sum(1.0 / r for r in ranks) / len(ranks)
    acc_at_1 = sum(1 for r in ranks if r == 1) / len(ranks)
    acc_at_3 = sum(1 for r in ranks if r <= 3) / len(ranks)
    mean_sim = sum(similarities) / len(similarities)

    return {
        "mrr": mrr,
        "acc@1": acc_at_1,
        "acc@3": acc_at_3,
        "mean_cosine_sim": mean_sim,
        "ranks": ranks,
        "similarities": similarities,
    }


def main():
    from sentence_transformers import SentenceTransformer

    nl_queries, short_queries, id_queries = load_benchmark()
    print(
        f"Benchmark: {len(nl_queries)} NL, {len(short_queries)} short, {len(id_queries)} identifier\n"
    )

    # Load models
    print("Loading base all-MiniLM-L12-v2...")
    base_model = SentenceTransformer("sentence-transformers/all-MiniLM-L12-v2")
    base_model.max_seq_length = 128

    print(f"Loading V6 from {V6_MODEL}...")
    v6_model = SentenceTransformer(str(V6_MODEL))
    v6_model.max_seq_length = 128

    # Evaluate both models
    for query_type, queries, label in [
        ("natural_language", nl_queries, "NL"),
        ("short_phrase", short_queries, "Short"),
        ("identifier", id_queries, "ID"),
    ]:
        if not queries:
            continue
        print(f"\n{'='*50}")
        print(f"  {label} queries ({len(queries)} items)")
        print(f"{'='*50}")

        base_result = evaluate_model(base_model, queries, "base")
        v6_result = evaluate_model(v6_model, queries, "V6")

        print(f"\n  {'Metric':<20} {'Base':>10} {'V6':>10} {'Delta':>10}")
        print(f"  {'-'*50}")
        for metric in ["mrr", "acc@1", "acc@3", "mean_cosine_sim"]:
            b = base_result[metric]
            v = v6_result[metric]
            delta = v - b
            marker = "✓" if delta > 0 else "✗" if delta < 0 else "="
            print(f"  {metric:<20} {b:>10.4f} {v:>10.4f} {delta:>+10.4f} {marker}")

        # Per-query detail for NL
        if query_type == "natural_language":
            print(f"\n  Per-query comparison:")
            print(f"  {'Query':<45} {'Base':>6} {'V6':>6} {'Winner':>8}")
            print(f"  {'-'*70}")
            for i, q in enumerate(queries):
                b_sim = base_result["similarities"][i]
                v_sim = v6_result["similarities"][i]
                winner = "V6" if v_sim > b_sim else "Base" if b_sim > v_sim else "Tie"
                query_short = q["query"][:43]
                print(f"  {query_short:<45} {b_sim:>6.3f} {v_sim:>6.3f} {winner:>8}")

    # Overall verdict
    base_nl = evaluate_model(base_model, nl_queries, "base")
    v6_nl = evaluate_model(v6_model, nl_queries, "V6")
    print(f"\n{'='*50}")
    print(f"  VERDICT")
    print(f"{'='*50}")
    if base_nl["mrr"] > v6_nl["mrr"]:
        print(f"  Base model NL MRR ({base_nl['mrr']:.4f}) > V6 ({v6_nl['mrr']:.4f})")
        print(f"  → V6 fine-tuning DAMAGED NL capabilities")
        print(f"  → V7 should start from BASE, not V6")
    else:
        print(f"  V6 NL MRR ({v6_nl['mrr']:.4f}) >= Base ({base_nl['mrr']:.4f})")
        print(f"  → V6 fine-tuning preserved or improved NL")
        print(f"  → V7 on V6 is valid")


if __name__ == "__main__":
    main()
