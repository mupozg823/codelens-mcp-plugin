#!/usr/bin/env python3
"""Retrieval F-beta(0.5) evaluator (SWE-grep pattern).

Scores a retrieval tool against the existing file/symbol labels and reports
weighted F-beta(0.5) at file-level and line-level granularity **alongside MRR**,
following the report conventions of role-retrieval.py / embedding-quality.py.

- Probes the already-running read-only daemon over HTTP (default
  http://127.0.0.1:7839) — read-only calls only, no mutation.
- Gold line for the strict line-level score is resolved from the authoritative
  ``find_symbol`` definition line of ``expected_symbol`` (datasets carry no
  line-range column, so we derive it; labels are never modified).

stdlib-only, matching the other benchmark scripts.
"""

import argparse
import json
import os
import sys
import time
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import benchmark_runtime_common as rc  # noqa: E402
import retrieval_fbeta_metrics as fb  # noqa: E402

DEFAULT_DATASET = "benchmarks/embedding-quality-dataset-self.json"


def parse_args():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--project",
        default=None,
        help="Bind retrieval to this project root. Default: the daemon's "
        "already-bound project (labels reference that indexed repo).",
    )
    p.add_argument("--base-url", default="http://127.0.0.1:7839")
    p.add_argument("--dataset", default=DEFAULT_DATASET)
    p.add_argument("--limit", type=int, default=30, help="Max queries to score")
    p.add_argument("--max-results", type=int, default=10)
    p.add_argument(
        "--retrieval-tool",
        default="bm25_symbol_search",
        help="Read-only tool returning ranked file/symbol rows",
    )
    p.add_argument("--beta", type=float, default=0.5)
    p.add_argument("--output", default=None, help="Optional JSON output path")
    return p.parse_args()


def load_rows(dataset_path: str, limit: int) -> list[dict]:
    raw = json.loads(Path(dataset_path).read_text(encoding="utf-8"))
    rows = raw.get("rows") if isinstance(raw, dict) else raw
    if not isinstance(rows, list) or not rows:
        raise SystemExit(f"dataset has no rows: {dataset_path}")
    return rows[:limit] if limit and limit > 0 else rows


def candidate_rows(payload: dict) -> list[dict]:
    """Normalize a tool payload into [{name, file, line}] ranked rows."""
    data = payload.get("data") if isinstance(payload.get("data"), dict) else payload
    if not isinstance(data, dict):
        return []
    raw = data.get("results") or data.get("symbols") or data.get("matches") or []
    out = []
    for row in raw:
        if not isinstance(row, dict):
            continue
        file_path = row.get("file_path") or row.get("file") or ""
        line = row.get("line")
        if line is None:
            line = row.get("start_line")
        out.append(
            {
                "name": row.get("name") or row.get("symbol_name"),
                "file": file_path,
                "line": line,
            }
        )
    return out


def first_rank(rows: list[dict], expected_symbol: str, suffix: str):
    for idx, row in enumerate(rows, start=1):
        if row.get("name") != expected_symbol:
            continue
        if suffix and not str(row.get("file", "")).endswith(suffix):
            continue
        return idx
    return None


class DaemonClient:
    def __init__(self, base_url: str):
        self.base_url = base_url
        self.session_id = None
        self.bound_project = None
        self._gold_line_cache: dict = {}

    def connect(self, project):
        try:
            sid, _, _ = rc.initialize_http_session(
                self.base_url, client_name="retrieval-fbeta"
            )
        except Exception as exc:  # noqa: BLE001
            raise SystemExit(
                f"ERROR: cannot reach daemon at {self.base_url}/mcp: {exc}\n"
                "Start the read-only daemon (see CLAUDE.md HTTP Daemon Operations)."
            )
        self.session_id = sid
        # Binding to the worktree path (a non-indexed checkout) silently zeroes
        # retrieval, so default to the daemon's already-active project.
        target = str(Path(project).resolve()) if project else self._active_project()
        self.bound_project = target
        if target:
            self.call("prepare_harness_session", {"project": target})

    def _active_project(self):
        payload = self.call("bm25_symbol_search", {"query": "a", "max_results": 1})
        data = payload.get("data") if isinstance(payload.get("data"), dict) else payload
        binding = data.get("project_binding") if isinstance(data, dict) else None
        if isinstance(binding, dict):
            return binding.get("active_project") or binding.get("session_project")
        return None

    def call(self, tool: str, args: dict) -> dict:
        resp = rc.mcp_http_tool_call(
            self.base_url, tool, args, session_id=self.session_id
        )
        return rc.extract_tool_payload(resp)

    def gold_line(self, expected_symbol: str, suffix: str):
        """Authoritative definition line of expected_symbol in the gold file."""
        key = (expected_symbol, suffix)
        if key in self._gold_line_cache:
            return self._gold_line_cache[key]
        line = None
        payload = self.call("find_symbol", {"name": expected_symbol})
        for row in candidate_rows(payload):
            if row.get("name") == expected_symbol and (
                not suffix or str(row.get("file", "")).endswith(suffix)
            ):
                line = row.get("line")
                break
        self._gold_line_cache[key] = line
        return line


def evaluate(client: DaemonClient, rows: list[dict], args) -> dict:
    per_query = []
    for item in rows:
        query = item["query"]
        expected_symbol = item.get("expected_symbol", "")
        suffix = item.get("expected_file_suffix", "")
        started = time.time()
        payload = client.call(
            args.retrieval_tool, {"query": query, "max_results": args.max_results}
        )
        elapsed_ms = (time.time() - started) * 1000.0
        candidates = candidate_rows(payload)

        rank = first_rank(candidates, expected_symbol, suffix)
        reciprocal_rank = 0.0 if rank is None else 1.0 / rank

        retrieved_files = {c["file"] for c in candidates if c["file"]}
        file_prf = fb.suffix_match_prf(retrieved_files, {suffix}, beta=args.beta)

        gold_line = client.gold_line(expected_symbol, suffix)
        retrieved_pairs = {
            (c["file"], c["line"]) for c in candidates if c["file"] and c["line"] is not None
        }
        # Only score line-level when the gold line is resolvable; otherwise a
        # vacuous empty-gold score would inflate recall.
        if suffix and gold_line is not None:
            line_prf = fb.line_pair_prf(
                retrieved_pairs, {(suffix, gold_line)}, beta=args.beta
            )
        else:
            line_prf = None

        per_query.append(
            {
                "query": query,
                "query_type": item.get("query_type", "uncategorized"),
                "expected_symbol": expected_symbol,
                "expected_file_suffix": suffix,
                "gold_line": gold_line,
                "rank": rank,
                "reciprocal_rank": reciprocal_rank,
                "candidate_count": len(candidates),
                "elapsed_ms": elapsed_ms,
                "file": file_prf,
                "line": line_prf,
            }
        )
    summary = fb.aggregate(per_query, beta=args.beta)
    summary["rows"] = per_query
    return summary


def render_markdown(summary: dict, args) -> str:
    lines = ["# Retrieval F-beta Summary", ""]
    lines.append(f"- Retrieval tool: `{args.retrieval_tool}`")
    lines.append(f"- Bound project: `{summary.get('bound_project')}`")
    lines.append(f"- Dataset: `{args.dataset}`")
    lines.append(f"- Queries scored: {summary['count']}")
    lines.append(f"- Beta: {summary['beta']} (precision-weighted)")
    lines.append("")
    lines.append("| Granularity | Scored | MRR | Precision | Recall | F-beta |")
    lines.append("| --- | --- | --- | --- | --- | --- |")
    lines.append(
        f"| file-level | {summary['count']} | {summary['mrr']:.3f} | "
        f"{summary['file_precision']:.3f} | {summary['file_recall']:.3f} | "
        f"{summary['file_f_beta']:.3f} |"
    )
    lines.append(
        f"| line-level | {summary['line_count']} | {summary['mrr']:.3f} | "
        f"{summary['line_precision']:.3f} | {summary['line_recall']:.3f} | "
        f"{summary['line_f_beta']:.3f} |"
    )
    return "\n".join(lines) + "\n"


def main():
    args = parse_args()
    rows = load_rows(args.dataset, args.limit)
    client = DaemonClient(args.base_url)
    client.connect(args.project)
    summary = evaluate(client, rows, args)
    summary["bound_project"] = client.bound_project
    print(render_markdown(summary, args))
    if args.output:
        out_path = Path(args.output)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(json.dumps(summary, indent=2), encoding="utf-8")
        print(f"Wrote {out_path}")


if __name__ == "__main__":
    main()
