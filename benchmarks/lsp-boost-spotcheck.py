#!/usr/bin/env python3
"""P1-4 caller wiring spot-check: measure lsp_boost=true vs baseline.

This is NOT the full 5-repo matrix ideally wanted for Task 2 — it is a
*honest* minimum-viable measurement whose purpose is to prove the
engine + MCP wiring fires end-to-end on a real repo with an installed
LSP server.

Hard constraints this script does not try to paper over:
  * The CLI one-shot path (`codelens-mcp project --cmd get_ranked_context`)
    spawns a fresh rust-analyzer per call. Cold start is 2-30s, so keep
    the query set tiny (≤ 10).
  * The probe only fires when the request carries `path` (needed to
    anchor `textDocument/references`). The standard embedding-quality
    datasets omit `path`, which is why they produce *zero* measured
    signal for lsp_boost — that is a documented blocker, not a bug.

Usage:
    python3 benchmarks/lsp-boost-spotcheck.py \\
        --output benchmarks/results/v1.9.50-lsp-boost-spotcheck-self.json
"""

from __future__ import annotations

import argparse
import json
import subprocess
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# Hand-crafted micro dataset: 5 self-repo queries that each point at a
# symbol whose actual cross-file callers are known. The `path` field
# anchors the LSP probe; `expected_file_suffix` pins the definition
# file so the baseline still ranks the exact match at the top.
DATASET = [
    {
        "query": "rank_symbols",
        "path": "crates/codelens-engine/src/symbols/ranking.rs",
        "expected_symbol": "rank_symbols",
        "expected_file_suffix": "crates/codelens-engine/src/symbols/ranking.rs",
    },
    {
        "query": "prune_to_budget",
        "path": "crates/codelens-engine/src/symbols/ranking.rs",
        "expected_symbol": "prune_to_budget",
        "expected_file_suffix": "crates/codelens-engine/src/symbols/ranking.rs",
    },
    {
        "query": "get_ranked_context_cached",
        "path": "crates/codelens-engine/src/symbols/reader.rs",
        "expected_symbol": "get_ranked_context_cached",
        "expected_file_suffix": "crates/codelens-engine/src/symbols/reader.rs",
    },
    {
        "query": "find_referencing_symbols_via_lsp",
        "path": "crates/codelens-engine/src/lsp/mod.rs",
        "expected_symbol": "find_referencing_symbols_via_lsp",
        "expected_file_suffix": "crates/codelens-engine/src/lsp/mod.rs",
    },
    {
        "query": "LspSessionPool",
        "path": "crates/codelens-engine/src/lsp/session.rs",
        "expected_symbol": "LspSessionPool",
        "expected_file_suffix": "crates/codelens-engine/src/lsp/session.rs",
    },
]

ARMS = (
    ("baseline", False),
    ("lsp_boost", True),
)


def parse_args():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--binary",
        default=str(REPO_ROOT / "target" / "release" / "codelens-mcp"),
    )
    p.add_argument("--project", default=str(REPO_ROOT))
    p.add_argument("--preset", default="balanced")
    p.add_argument("--output", required=True)
    p.add_argument("--timeout", type=int, default=120)
    return p.parse_args()


def run_tool(
    binary: str, project: str, preset: str, cmd: str, args: dict, timeout: int
):
    argv = [
        binary,
        project,
        "--preset",
        preset,
        "--cmd",
        cmd,
        "--args",
        json.dumps(args),
    ]
    t0 = time.perf_counter()
    r = subprocess.run(argv, capture_output=True, text=True, timeout=timeout)
    elapsed_ms = round((time.perf_counter() - t0) * 1000, 2)
    out = (r.stdout or "").strip()
    payload = None
    if out:
        try:
            payload = json.loads(out)
        except Exception:
            try:
                payload = json.loads(out.splitlines()[-1])
            except Exception:
                payload = None
    return {
        "returncode": r.returncode,
        "elapsed_ms": elapsed_ms,
        "payload": payload,
        "stderr": (r.stderr or "").strip(),
    }


def top_symbols(payload, limit: int = 5):
    if not isinstance(payload, dict):
        return []
    data = payload.get("data") if isinstance(payload.get("data"), dict) else payload
    symbols = data.get("symbols") if isinstance(data, dict) else None
    if not isinstance(symbols, list):
        return []
    out = []
    for row in symbols[:limit]:
        out.append(
            {
                "name": row.get("name"),
                "file": row.get("file"),
                "relevance_score": row.get("relevance_score"),
            }
        )
    return out


def rank_of(symbol, file_suffix, rows):
    for idx, row in enumerate(rows, start=1):
        if row.get("name") != symbol:
            continue
        if file_suffix and not str(row.get("file", "")).endswith(file_suffix):
            continue
        return idx
    return None


def main() -> int:
    args = parse_args()
    arms_output = []
    for arm_name, lsp_boost in ARMS:
        rows = []
        for item in DATASET:
            tool_args = {
                "query": item["query"],
                "path": item["path"],
                "max_tokens": 1200,
                "include_body": False,
                "lsp_boost": lsp_boost,
            }
            r = run_tool(
                args.binary,
                args.project,
                args.preset,
                "get_ranked_context",
                tool_args,
                args.timeout,
            )
            payload = r["payload"]
            all_symbols = []
            if isinstance(payload, dict):
                data = (
                    payload.get("data")
                    if isinstance(payload.get("data"), dict)
                    else payload
                )
                all_symbols = data.get("symbols", []) if isinstance(data, dict) else []
            rank = rank_of(
                item["expected_symbol"],
                item.get("expected_file_suffix"),
                all_symbols,
            )
            rows.append(
                {
                    "query": item["query"],
                    "path": item["path"],
                    "expected_symbol": item["expected_symbol"],
                    "expected_file_suffix": item.get("expected_file_suffix"),
                    "rank": rank,
                    "elapsed_ms": r["elapsed_ms"],
                    "returncode": r["returncode"],
                    "top": top_symbols(payload, 5),
                    "candidate_count": len(all_symbols),
                    "stderr_excerpt": r["stderr"][-400:],
                }
            )
        reciprocal = [1.0 / row["rank"] for row in rows if row["rank"] is not None]
        arms_output.append(
            {
                "arm": arm_name,
                "lsp_boost": lsp_boost,
                "mrr": sum(reciprocal) / len(rows) if rows else 0.0,
                "hits": len(reciprocal),
                "rows": rows,
            }
        )

    result = {
        "schema_version": 1,
        "project": str(args.project),
        "binary": str(args.binary),
        "preset": args.preset,
        "dataset_size": len(DATASET),
        "arms": arms_output,
    }
    out_path = Path(args.output)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(result, indent=2, ensure_ascii=False))
    for arm in arms_output:
        print(
            f"[{arm['arm']} lsp_boost={arm['lsp_boost']}] "
            f"MRR={arm['mrr']:.4f} hits={arm['hits']}/{len(DATASET)}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
