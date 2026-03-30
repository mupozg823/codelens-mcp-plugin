"""
CodeLens Embedding Model Benchmark
Compare BGE-Small-EN-v1.5 (current) vs MiniLM-L12 fine-tuned on CodeSearchNet
"""

import time
import json
import numpy as np
from pathlib import Path

from sentence_transformers import SentenceTransformer

# ── Test queries and code snippets ──────────────────────────────────
# Pairs: (natural language query, expected matching code, language)
TEST_PAIRS = [
    (
        "sort a list of numbers in ascending order",
        "def sort_numbers(nums):\n    return sorted(nums)",
        "python",
    ),
    (
        "read a file and return its contents as string",
        "fn read_file(path: &str) -> String {\n    std::fs::read_to_string(path).unwrap()\n}",
        "rust",
    ),
    (
        "make an HTTP GET request",
        "async function fetchData(url) {\n  const response = await fetch(url);\n  return response.json();\n}",
        "javascript",
    ),
    (
        "find the maximum value in an array",
        "func maxVal(arr []int) int {\n    m := arr[0]\n    for _, v := range arr {\n        if v > m { m = v }\n    }\n    return m\n}",
        "go",
    ),
    (
        "connect to a database",
        "import sqlite3\ndef connect_db(path):\n    conn = sqlite3.connect(path)\n    return conn",
        "python",
    ),
    (
        "parse JSON string into object",
        "public static JsonObject parseJson(String json) {\n    return JsonParser.parseString(json).getAsJsonObject();\n}",
        "java",
    ),
    (
        "calculate fibonacci sequence",
        "def fibonacci(n):\n    if n <= 1:\n        return n\n    return fibonacci(n-1) + fibonacci(n-2)",
        "python",
    ),
    (
        "create a TCP server that listens on a port",
        'use std::net::TcpListener;\nfn start_server(port: u16) {\n    let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();\n    for stream in listener.incoming() { /* handle */ }\n}',
        "rust",
    ),
]

# Distractors (code that should NOT match queries)
DISTRACTORS = [
    "class Color:\n    RED = 1\n    GREEN = 2\n    BLUE = 3",
    "fn draw_circle(ctx: &Context, x: f64, y: f64, r: f64) {\n    ctx.arc(x, y, r, 0.0, 2.0 * PI);\n}",
    "export const LOGO_URL = 'https://example.com/logo.png';",
    "type Config struct {\n    Host string\n    Port int\n    Debug bool\n}",
    "SELECT u.name, u.email FROM users u WHERE u.active = 1",
    "void playSound(String filename) {\n    AudioClip clip = AudioSystem.getAudioClip(filename);\n    clip.play();\n}",
    "@keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }",
    "def send_email(to, subject, body):\n    msg = MIMEText(body)\n    msg['Subject'] = subject\n    smtp.send_message(msg)",
]


def cosine_sim(a, b):
    return np.dot(a, b) / (np.linalg.norm(a) * np.linalg.norm(b))


def evaluate_model(model_name, model):
    """Evaluate a model on code search quality."""
    print(f"\n{'='*60}")
    print(f"Model: {model_name}")
    print(f"{'='*60}")

    # Embed all code snippets (correct + distractors)
    all_code = [code for _, code, _ in TEST_PAIRS] + DISTRACTORS

    t0 = time.perf_counter()
    code_embeddings = model.encode(all_code, show_progress_bar=False)
    code_time = time.perf_counter() - t0

    queries = [q for q, _, _ in TEST_PAIRS]
    t0 = time.perf_counter()
    query_embeddings = model.encode(queries, show_progress_bar=False)
    query_time = time.perf_counter() - t0

    # Calculate metrics
    mrr_sum = 0.0
    top1_correct = 0
    top3_correct = 0

    for i, (query, expected_code, lang) in enumerate(TEST_PAIRS):
        # Compute similarity to all code snippets
        sims = [cosine_sim(query_embeddings[i], ce) for ce in code_embeddings]
        # Rank by similarity (descending)
        ranked_indices = np.argsort(sims)[::-1]

        # Find rank of correct answer (index i in all_code)
        rank = np.where(ranked_indices == i)[0][0] + 1

        mrr_sum += 1.0 / rank
        if rank == 1:
            top1_correct += 1
        if rank <= 3:
            top3_correct += 1

        status = "O" if rank == 1 else "X"
        print(f"  [{status}] rank={rank:2d} | {query[:50]}")

    n = len(TEST_PAIRS)
    mrr = mrr_sum / n
    acc1 = top1_correct / n
    acc3 = top3_correct / n
    avg_latency = ((code_time + query_time) / (len(all_code) + len(queries))) * 1000

    print(f"\n  MRR:      {mrr:.3f}")
    print(f"  Acc@1:    {acc1:.1%} ({top1_correct}/{n})")
    print(f"  Acc@3:    {acc3:.1%} ({top3_correct}/{n})")
    print(f"  Latency:  {avg_latency:.1f}ms per embedding")
    print(f"  Code emb: {code_time*1000:.0f}ms for {len(all_code)} snippets")
    print(f"  Query emb: {query_time*1000:.0f}ms for {len(queries)} queries")

    return {
        "model": model_name,
        "mrr": mrr,
        "acc1": acc1,
        "acc3": acc3,
        "latency_ms": avg_latency,
    }


def get_model_size_mb(model_path):
    """Get total model file size in MB."""
    total = 0
    p = Path(model_path)
    if p.is_dir():
        for f in p.rglob("*"):
            if f.is_file():
                total += f.stat().st_size
    return total / (1024 * 1024)


def main():
    results = []

    # ── Model 1: BGE-Small-EN-v1.5 (current CodeLens model) ──
    print("Loading BGE-Small-EN-v1.5 (current)...")
    bge = SentenceTransformer("BAAI/bge-small-en-v1.5")
    r1 = evaluate_model("BGE-Small-EN-v1.5 (current)", bge)
    r1["size_mb"] = get_model_size_mb(
        bge._model_card_vars.get("model_path", bge.model_card_data.model_id or "")
    )
    results.append(r1)
    del bge

    # ── Model 2: MiniLM-L12 fine-tuned on CodeSearchNet ──
    print("\nLoading MiniLM-L12 CodeSearchNet...")
    minilm = SentenceTransformer("isuruwijesiri/all-MiniLM-L12-v2-code-search-512")
    r2 = evaluate_model("MiniLM-L12-CodeSearchNet", minilm)
    results.append(r2)
    del minilm

    # ── Model 3: Base MiniLM-L6 (no code training, baseline) ──
    print("\nLoading MiniLM-L6-v2 (base, no code training)...")
    base = SentenceTransformer("sentence-transformers/all-MiniLM-L6-v2")
    r3 = evaluate_model("MiniLM-L6-v2 (base)", base)
    results.append(r3)
    del base

    # ── Summary ──
    print(f"\n{'='*60}")
    print("SUMMARY")
    print(f"{'='*60}")
    print(f"{'Model':<35} {'MRR':>6} {'Acc@1':>6} {'Acc@3':>6} {'ms/emb':>7}")
    print("-" * 65)
    for r in results:
        print(
            f"{r['model']:<35} {r['mrr']:>6.3f} {r['acc1']:>5.0%} {r['acc3']:>5.0%} {r['latency_ms']:>6.1f}"
        )

    with open("benchmark_results.json", "w") as f:
        json.dump(results, f, indent=2)
    print("\nResults saved to benchmark_results.json")


if __name__ == "__main__":
    main()
