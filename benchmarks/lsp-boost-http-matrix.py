#!/usr/bin/env python3
"""P1-4 HTTP daemon lsp_boost matrix with optional LSP prewarm.

Differences from `lsp-boost-spotcheck.py`:
  * Keeps a single persistent daemon for the whole run so the LSP
    session pool survives across queries (CLI one-shot spawns a cold
    rust-analyzer per query and returned 0 refs in ~7 ms).
  * With `--warm-lsp`, issues one `find_referencing_symbols` call per
    distinct `(path, query)` *before* the measurement arms run. That
    gives rust-analyzer / pyright / tsserver time to index the
    workspace before the real ranked-context call asks it for refs.

The per-ref ranking change (commit a9e23f1) rewards the actual
container of the most refs. Warm LSP tends to return tighter refs
than tree-sitter text search (which matches any lexical hit),
so this script exists to measure how much of the per-ref uplift
lives in tree-sitter alone vs what LSP can add on top.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import benchmark_runtime_common as rc  # noqa: E402


REPO_ROOT = Path(__file__).resolve().parent.parent


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--binary",
        default=str(REPO_ROOT / "target" / "release" / "codelens-mcp"),
        help="Path to codelens-mcp; the daemon helper will swap in the "
        "`-http` sibling if present.",
    )
    p.add_argument("--project", required=True)
    p.add_argument("--dataset", required=True)
    p.add_argument("--output", required=True)
    p.add_argument("--preset", default="full")
    p.add_argument(
        "--warm-lsp",
        action="store_true",
        help="Fire an LSP-only find_referencing_symbols probe per path "
        "before the arms run so rust-analyzer / pyright / tsserver has "
        "time to index the workspace.",
    )
    p.add_argument(
        "--warm-timeout",
        type=int,
        default=90,
        help="Per-prewarm timeout in seconds (default 90).",
    )
    p.add_argument(
        "--auto-attach",
        action="store_true",
        help="Instead of issuing explicit find_referencing_symbols probes, "
        "rely on `prepare_harness_session`'s `CODELENS_LSP_AUTO=true` "
        "auto-attach to fire a fire-and-forget workspace_symbols warm-up "
        "and then wait `--auto-attach-wait` seconds for indexing before "
        "the measurement arms run. Measures whether an agent session that "
        "calls `prepare_harness_session` once gets the same uplift as the "
        "explicit per-query prewarm.",
    )
    p.add_argument(
        "--auto-attach-wait",
        type=int,
        default=45,
        help="Seconds to sleep after prepare_harness_session before the "
        "measurement arms start. Default 45 covers pyright / tsserver "
        "indexing on Flask / Zod; rust-analyzer on self needs ~15-30s.",
    )
    return p.parse_args()


def load_dataset(path: str) -> list[dict]:
    rows = json.loads(Path(path).read_text(encoding="utf-8"))
    if not isinstance(rows, list):
        raise SystemExit("dataset must be a JSON list")
    for idx, row in enumerate(rows):
        for key in ("query", "path", "expected_symbol"):
            if key not in row:
                raise SystemExit(f"row {idx} missing required key: {key}")
    return rows


def rank_of(
    expected_symbol: str, file_suffix: str | None, rows: list[dict]
) -> int | None:
    for idx, row in enumerate(rows, start=1):
        if row.get("name") != expected_symbol:
            continue
        if file_suffix and not str(row.get("file", "")).endswith(file_suffix):
            continue
        return idx
    return None


def top_symbols(symbols: list[dict], limit: int = 5) -> list[dict]:
    return [
        {
            "name": s.get("name"),
            "file": s.get("file"),
            "relevance_score": s.get("relevance_score"),
        }
        for s in symbols[:limit]
    ]


def warm_lsp(base_url: str, session_id: str, dataset: list[dict], timeout: int) -> dict:
    """Fire one LSP refs probe per distinct (path, query) tuple.

    Returns a small report summarising how many probes actually produced
    LSP-backed references (vs tree-sitter fallback). Useful for the
    honesty section of the matrix markdown.
    """
    warmed: dict[str, dict] = {}
    for idx, item in enumerate(dataset):
        key = f"{item['path']}::{item['query']}"
        if key in warmed:
            continue
        # Resolve the anchor (line, column) via find_symbol.
        sym_response = rc.mcp_http_tool_call(
            base_url,
            "find_symbol",
            {
                "name": item["query"],
                "file_path": item["path"],
                "exact_match": True,
                "max_matches": 1,
            },
            request_id=1000 + idx,
            session_id=session_id,
            timeout_seconds=30,
        )
        sym_payload = rc.extract_tool_payload(sym_response)
        data = (
            sym_payload.get("data")
            if isinstance(sym_payload.get("data"), dict)
            else sym_payload
        )
        symbols = data.get("symbols", []) if isinstance(data, dict) else []
        if not symbols:
            warmed[key] = {"status": "no_anchor"}
            continue
        line, column = symbols[0].get("line", 0), symbols[0].get("column", 0)
        if line <= 0:
            warmed[key] = {"status": "no_line"}
            continue
        # LSP refs with union=true → triggers the pool to bring up rust-analyzer
        # and merges tree-sitter for the first call. Subsequent get_ranked_context
        # calls in the same daemon session reuse the warm pool.
        refs_response = rc.mcp_http_tool_call(
            base_url,
            "find_referencing_symbols",
            {
                "file_path": item["path"],
                "line": line,
                "column": column,
                "use_lsp": True,
                "union": True,
                "max_results": 32,
            },
            request_id=2000 + idx,
            session_id=session_id,
            timeout_seconds=timeout,
        )
        refs_payload = rc.extract_tool_payload(refs_response)
        refs_data = (
            refs_payload.get("data")
            if isinstance(refs_payload.get("data"), dict)
            else refs_payload
        )
        sources = refs_data.get("sources", {}) if isinstance(refs_data, dict) else {}
        warmed[key] = {
            "status": "ok",
            "backend": (
                refs_data.get("backend") if isinstance(refs_data, dict) else None
            ),
            "count": refs_data.get("count") if isinstance(refs_data, dict) else None,
            "sources": sources,
        }
    return warmed


def extract_symbols(response: dict) -> list[dict]:
    """Pull the ranked-context symbol list out of the HTTP MCP envelope.

    `extract_tool_payload` parses `content[0].text`, which the adaptive
    compression pipeline may have already shrunk to a skeleton
    (`{success, truncated, compression_stage, error, token_estimate}`).
    The full payload survives in `structuredContent.data.symbols`, so
    prefer that and fall back to text only when the server did not
    attach a structured version.
    """
    if not isinstance(response, dict):
        return []
    result = response.get("result")
    if not isinstance(result, dict):
        return []
    structured = result.get("structuredContent")
    if isinstance(structured, dict):
        data = structured.get("data")
        if isinstance(data, dict):
            syms = data.get("symbols")
            if isinstance(syms, list):
                return syms
        syms = structured.get("symbols")
        if isinstance(syms, list):
            return syms
    # Fallback: parse the text content even if it may have been compressed.
    content = result.get("content")
    if isinstance(content, list) and content:
        text = content[0].get("text", "{}")
        try:
            parsed = json.loads(text)
        except Exception:
            return []
        data = parsed.get("data") if isinstance(parsed.get("data"), dict) else parsed
        if isinstance(data, dict):
            syms = data.get("symbols")
            if isinstance(syms, list):
                return syms
    return []


def run_arm(
    base_url: str,
    session_id: str,
    dataset: list[dict],
    lsp_boost: bool,
    request_base: int,
) -> dict:
    rows = []
    for idx, item in enumerate(dataset):
        args = {
            "query": item["query"],
            "path": item["path"],
            "max_tokens": 1200,
            "include_body": False,
            "lsp_boost": lsp_boost,
        }
        t0 = time.perf_counter()
        response = rc.mcp_http_tool_call(
            base_url,
            "get_ranked_context",
            args,
            request_id=request_base + idx,
            session_id=session_id,
            timeout_seconds=120,
        )
        elapsed_ms = round((time.perf_counter() - t0) * 1000, 2)
        symbols = extract_symbols(response)
        rank = rank_of(
            item["expected_symbol"],
            item.get("expected_file_suffix"),
            symbols,
        )
        rows.append(
            {
                "query": item["query"],
                "expected_symbol": item["expected_symbol"],
                "rank": rank,
                "elapsed_ms": elapsed_ms,
                "candidate_count": len(symbols),
                "top": top_symbols(symbols),
            }
        )
    reciprocal = [1.0 / row["rank"] for row in rows if row["rank"] is not None]
    return {
        "lsp_boost": lsp_boost,
        "mrr": sum(reciprocal) / len(rows) if rows else 0.0,
        "hits": len(reciprocal),
        "rows": rows,
    }


def main() -> int:
    args = parse_args()
    dataset = load_dataset(args.dataset)

    # --auto-attach relies on `prepare_harness_session`'s fire-and-forget
    # LSP warm-up which only runs when `CODELENS_LSP_AUTO=true` is set on
    # the daemon process. We set it on the parent environment so the
    # subprocess inherits.
    if args.auto_attach:
        os.environ["CODELENS_LSP_AUTO"] = "true"

    base_url, port, proc = rc.start_http_daemon(
        args.binary, args.project, preset=args.preset
    )
    if not base_url:
        rc.stop_http_daemon(proc)
        stderr = proc.stderr.read() if proc and proc.stderr else ""
        raise SystemExit(f"HTTP daemon failed to start. stderr:\n{stderr[-600:]}")

    warm_report: dict = {}
    try:
        session_id, _init_payload, _init_headers = rc.initialize_http_session(
            base_url,
            timeout_seconds=20,
        )
        if not session_id:
            raise SystemExit("HTTP initialize did not return a session id")

        # Make sure the project is indexed — the first call after a fresh
        # daemon often finds an empty DB on external repos.
        rc.mcp_http_tool_call(
            base_url,
            "refresh_symbol_index",
            {},
            request_id=1,
            session_id=session_id,
            timeout_seconds=300,
        )

        if args.auto_attach:
            # One-shot bootstrap the way an agent session would. The
            # daemon fires a fire-and-forget `search_workspace_symbols`
            # per detected language; we then sleep to let the LSP index
            # the workspace before the arms ask it for refs.
            auto_resp = rc.mcp_http_tool_call(
                base_url,
                "prepare_harness_session",
                {},
                request_id=2,
                session_id=session_id,
                timeout_seconds=30,
            )
            auto_payload = rc.extract_tool_payload(auto_resp)
            auto_data = (
                auto_payload.get("data")
                if isinstance(auto_payload.get("data"), dict)
                else auto_payload
            )
            warm_report = {
                "mode": "auto_attach",
                "lsp_auto_attach": (
                    auto_data.get("lsp_auto_attach")
                    if isinstance(auto_data, dict)
                    else None
                ),
                "wait_seconds": args.auto_attach_wait,
            }
            if args.auto_attach_wait > 0:
                time.sleep(args.auto_attach_wait)

        if args.warm_lsp:
            warm_report = warm_lsp(base_url, session_id, dataset, args.warm_timeout)

        baseline = run_arm(base_url, session_id, dataset, False, request_base=5000)
        boosted = run_arm(base_url, session_id, dataset, True, request_base=6000)
    finally:
        rc.stop_http_daemon(proc)

    result = {
        "schema_version": 1,
        "project": str(args.project),
        "binary": str(args.binary),
        "dataset_path": str(args.dataset),
        "dataset_size": len(dataset),
        "preset": args.preset,
        "warm_lsp": bool(args.warm_lsp),
        "warm_report": warm_report,
        "arms": [
            {"arm": "baseline", **baseline},
            {"arm": "lsp_boost", **boosted},
        ],
    }
    out = Path(args.output)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(result, indent=2, ensure_ascii=False))
    for arm in result["arms"]:
        print(
            f"[{arm['arm']} lsp_boost={arm['lsp_boost']}] "
            f"MRR={arm['mrr']:.4f} hits={arm['hits']}/{len(dataset)}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
