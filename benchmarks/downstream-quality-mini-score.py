#!/usr/bin/env python3
"""Label-based scoring for Downstream Quality Mini A/B.
Matches each required label substring against the subagent's answer.
Prints per-cell, per-question, and aggregate results."""
import json
from pathlib import Path

base = Path(
    "/Users/bagjaeseog/codelens-mcp-plugin/benchmarks/downstream-quality-mini-dataset.json"
)
ds = json.loads(base.read_text())
answers = json.loads(Path("/tmp/dq-mini/answers.json").read_text())

rows = []
for q in ds["questions"]:
    qid = q["id"].split("_")[0]  # Q1, Q2, Q3
    labels = q["labels"]
    for ctx in ("A", "B"):
        ans = answers[f"{qid}_{ctx}"]
        hits = [lbl for lbl in labels if lbl in ans]
        missed = [lbl for lbl in labels if lbl not in ans]
        rows.append(
            {
                "cell": f"{qid}_{ctx}",
                "difficulty": q["difficulty"],
                "label_count": len(labels),
                "matched": len(hits),
                "accuracy": round(len(hits) / len(labels), 3),
                "hits": hits,
                "missed": missed,
            }
        )

# Print per-cell
print(f"{'cell':8} {'diff':6} {'matched':>10} {'accuracy':>10}  missed")
print("-" * 90)
for r in rows:
    print(
        f"{r['cell']:8} {r['difficulty']:6} {r['matched']}/{r['label_count']:<4}     {r['accuracy']:6.2f}     {r['missed']}"
    )

# Aggregate by context
print()
by_ctx = {"A": [], "B": []}
for r in rows:
    by_ctx[r["cell"][-1]].append(r)
for ctx in ("A", "B"):
    labels_total = sum(r["label_count"] for r in by_ctx[ctx])
    matched_total = sum(r["matched"] for r in by_ctx[ctx])
    acc = matched_total / labels_total if labels_total else 0
    print(f"ctx_{ctx}: matched {matched_total}/{labels_total}  accuracy {acc:.3f}")

# Token ratios
print()
import os

for qid in ("Q1", "Q2", "Q3"):
    sa = os.path.getsize(f"/tmp/dq-mini/{qid}_ctxA.txt")
    sb = os.path.getsize(f"/tmp/dq-mini/{qid}_ctxB.json")
    print(
        f"{qid}: ctx_A {sa:>6} bytes / ctx_B {sb:>6} bytes  (ratio A/B = {sa/sb:.2f})"
    )
