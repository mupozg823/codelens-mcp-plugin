#!/usr/bin/env python3
# noqa: SIZE_OK - standalone benchmark CLI; split after P3 gates stabilize.
"""Embedding quality benchmark for the actual CodeLens runtime."""

import argparse
import collections
import concurrent.futures
import hashlib
import json
import math
import os
import signal
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

from benchmark_runtime_common import (
    parse_output_json,
    resolve_codelens_model_dir,
    tool_payload_succeeded,
    validate_expected_file_suffixes,
)

METHOD_ORDER: tuple[str, ...] = (
    "semantic_search",
    "get_ranked_context_no_semantic",
    "get_ranked_context",
    "bm25_symbol_search",
)
MAX_WORKERS = 16
MAX_BATCH_SIZE = 64
MAX_METHOD_WORKERS = len(METHOD_ORDER)


def positive_int(raw: str) -> int:
    try:
        value = int(raw)
    except ValueError as exc:
        raise argparse.ArgumentTypeError("value must be a positive integer") from exc
    if value < 1:
        raise argparse.ArgumentTypeError("value must be a positive integer")
    return value


def bounded_positive_int(raw: str, label: str, maximum: int) -> int:
    value = positive_int(raw)
    if value > maximum:
        raise argparse.ArgumentTypeError(f"{label} must be <= {maximum}")
    return value


def worker_count(raw: str) -> int:
    return bounded_positive_int(raw, "workers", MAX_WORKERS)


def batch_size(raw: str) -> int:
    return bounded_positive_int(raw, "batch-size", MAX_BATCH_SIZE)


def method_worker_count(raw: str) -> int:
    return bounded_positive_int(raw, "method-workers", MAX_METHOD_WORKERS)


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("project_path", nargs="?", default=".")
    parser.add_argument(
        "--binary",
        default=os.environ.get(
            "CODELENS_BIN",
            os.path.join(
                os.path.dirname(__file__), "..", "target", "debug", "codelens-mcp"
            ),
        ),
    )
    parser.add_argument(
        "--dataset",
        default=os.path.join(
            os.path.dirname(__file__), "embedding-quality-dataset-self.json"
        ),
    )
    parser.add_argument("--preset", default="balanced")
    parser.add_argument("--output", default="benchmarks/embedding-quality-results.json")
    parser.add_argument(
        "--stdout",
        choices=("json", "summary", "none"),
        default="json",
        help="Control benchmark stdout. Use 'summary' in CI/agent runs to avoid emitting the full JSON artifact.",
    )
    parser.add_argument("--markdown-output", default="")
    parser.add_argument(
        "--triage-output",
        default="",
        help="Optional machine-readable ranker triage artifact with candidate misses, semantic-hit demotions, token budget, and cache evidence",
    )
    parser.add_argument("--max-results", type=int, default=10)
    parser.add_argument(
        "--tool-timeout",
        type=int,
        default=180,
        help=(
            "Seconds before a single tool invocation is recorded as a timeout "
            "failure. Batch mode scales this by batch size."
        ),
    )
    parser.add_argument(
        "--ranked-context-max-tokens",
        type=int,
        default=50000,
        help="max_tokens for get_ranked_context benchmark lanes; keep above the hybrid retrieval floor to avoid truncation failures",
    )
    parser.add_argument(
        "--embed-model",
        default=os.environ.get("CODELENS_EMBED_MODEL", ""),
        help="Override CODELENS_EMBED_MODEL for this benchmark run",
    )
    parser.add_argument("--isolated-copy", action="store_true")
    parser.add_argument("--keep-isolated-copy", action="store_true")
    parser.add_argument(
        "--check",
        action="store_true",
        help="Exit 1 if hybrid/lexical MRR floors are not met",
    )
    parser.add_argument(
        "--min-hybrid-mrr",
        type=float,
        default=0.65,
        help="Floor for hybrid MRR@N under --check (default 0.65; self baseline v1.9.36 = 0.681)",
    )
    parser.add_argument(
        "--min-lexical-mrr",
        type=float,
        default=0.50,
        help="Floor for lexical MRR@N under --check",
    )
    parser.add_argument(
        "--min-hybrid-recall",
        type=float,
        default=0.0,
        help="Optional floor for hybrid Recall@N under --check; 0 disables",
    )
    parser.add_argument(
        "--min-hybrid-acc1",
        type=float,
        default=0.0,
        help="Optional floor for hybrid Acc@1 under --check; 0 disables",
    )
    parser.add_argument(
        "--min-hybrid-mrr-by-query-type",
        action="append",
        default=[],
        metavar="QUERY_TYPE=FLOOR",
        help="Optional per-query-type hybrid MRR floor under --check; repeatable, e.g. natural_language=0.45",
    )
    parser.add_argument(
        "--min-hybrid-recall-by-query-type",
        action="append",
        default=[],
        metavar="QUERY_TYPE=FLOOR",
        help="Optional per-query-type hybrid Recall@N floor under --check; repeatable",
    )
    parser.add_argument(
        "--min-hybrid-acc1-by-query-type",
        action="append",
        default=[],
        metavar="QUERY_TYPE=FLOOR",
        help="Optional per-query-type hybrid Acc@1 floor under --check; repeatable",
    )
    parser.add_argument(
        "--max-hybrid-demoted-semantic-hits",
        type=int,
        default=-1,
        help="Optional ceiling for semantic hits dropped or demoted by hybrid ranker under --check; -1 disables",
    )
    parser.add_argument(
        "--max-hybrid-candidate-missing-rate",
        type=float,
        default=-1.0,
        help="Optional ceiling for hybrid candidate_missing rate under --check; -1 disables",
    )
    parser.add_argument(
        "--max-hybrid-avg-ms",
        type=float,
        default=0.0,
        help="Optional ceiling for hybrid average latency under --check; 0 disables",
    )
    parser.add_argument(
        "--max-hybrid-avg-response-bytes",
        type=int,
        default=0,
        help="Optional ceiling for hybrid average compact JSON payload bytes under --check; 0 disables",
    )
    parser.add_argument(
        "--max-hybrid-p95-response-tokens",
        type=int,
        default=0,
        help="Optional ceiling for hybrid p95 estimated response tokens under --check; 0 disables",
    )
    parser.add_argument(
        "--methods",
        default="all",
        help=(
            "Comma-separated methods to run for iterative tuning. "
            "Use 'all' for the full gate. Must include get_ranked_context."
        ),
    )
    parser.add_argument(
        "--workers",
        type=worker_count,
        default=1,
        help=(
            "Parallel row workers per method for benchmark subprocess calls. "
            f"Default 1 preserves sequential execution; max {MAX_WORKERS}."
        ),
    )
    parser.add_argument(
        "--method-workers",
        type=method_worker_count,
        default=1,
        help=(
            "Parallel comparator method workers. Default 1 preserves sequential "
            f"method evaluation; max {MAX_METHOD_WORKERS}."
        ),
    )
    parser.add_argument(
        "--batch-size",
        type=batch_size,
        default=1,
        help=(
            "Tool calls per codelens-mcp subprocess. "
            f"Default 1 preserves one process per benchmark row; max {MAX_BATCH_SIZE}."
        ),
    )
    parser.add_argument(
        "--query-cache-probe",
        choices=("on", "off"),
        default="on",
        help=(
            "Run the extra two-call get_ranked_context cache probe. "
            "Use 'off' for fast ranker iteration."
        ),
    )
    return parser.parse_args()


ARGS = parse_args()
SOURCE_PROJECT = os.path.abspath(ARGS.project_path)
PROJECT = SOURCE_PROJECT
BIN = os.path.abspath(ARGS.binary)
DATASET = os.path.abspath(ARGS.dataset)
RUN_ENV = os.environ.copy()
if ARGS.embed_model:
    RUN_ENV["CODELENS_EMBED_MODEL"] = ARGS.embed_model
if "CODELENS_MODEL_DIR" not in RUN_ENV:
    repo_model_dir = (
        Path(__file__).resolve().parent.parent / "crates" / "codelens-engine" / "models"
    )
    if resolve_codelens_model_dir(BIN, env={"CODELENS_MODEL_DIR": str(repo_model_dir)}):
        RUN_ENV["CODELENS_MODEL_DIR"] = str(repo_model_dir)


def parse_requested_methods(raw: str) -> list[str]:
    text = (raw or "all").strip()
    if text == "all":
        return list(METHOD_ORDER)
    requested = []
    for part in text.split(","):
        method = part.strip()
        if not method:
            continue
        if method not in METHOD_ORDER:
            valid = ", ".join(METHOD_ORDER)
            raise SystemExit(f"unknown benchmark method: {method}; valid: {valid}")
        if method not in requested:
            requested.append(method)
    if "get_ranked_context" not in requested:
        raise SystemExit("--methods must include get_ranked_context")
    return requested


def compute_file_sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def resolve_runtime_model_dir():
    return resolve_codelens_model_dir(
        BIN,
        env=RUN_ENV,
        repo_root=Path(__file__).resolve().parent.parent,
    )


def payload_data(payload):
    return payload.get("data") if isinstance(payload.get("data"), dict) else payload


def snapshot_runtime_model(capabilities_payload=None):
    data = payload_data(capabilities_payload or {}) or {}
    runtime = {
        "embedding_model": data.get("embedding_model"),
        "runtime_preference": data.get("embedding_runtime_preference"),
        "backend": data.get("embedding_runtime_backend"),
        "threads": data.get("embedding_threads"),
        "max_length": data.get("embedding_max_length"),
        "fallback_reason": data.get("embedding_runtime_fallback_reason"),
        "requested_embed_model": ARGS.embed_model or None,
    }
    model_dir = resolve_runtime_model_dir()
    if model_dir is None:
        return runtime
    model_path = model_dir / "model.onnx"
    config_path = model_dir / "config.json"
    config = (
        json.loads(config_path.read_text(encoding="utf-8"))
        if config_path.exists()
        else {}
    )
    runtime.update(
        {
            "model_dir": str(model_dir),
            "model_path": str(model_path),
            "config_path": str(config_path),
            "sha256": compute_file_sha256(model_path),
            "size_bytes": model_path.stat().st_size,
            "num_hidden_layers": config.get("num_hidden_layers"),
            "hidden_size": config.get("hidden_size"),
        }
    )
    return runtime


def subprocess_text(value):
    if value is None:
        return ""
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    return str(value)


def build_tool_result(elapsed_ms, returncode, payload, stderr, raw_output=""):
    if payload is None:
        response_bytes = len(raw_output.encode("utf-8"))
    else:
        compact_payload = json.dumps(
            payload, ensure_ascii=False, separators=(",", ":")
        )
        response_bytes = len(compact_payload.encode("utf-8"))
    return {
        "elapsed_ms": elapsed_ms,
        "batch_amortized_elapsed_ms": None,
        "response_bytes": response_bytes,
        "estimated_response_tokens": max(1, response_bytes // 4),
        "returncode": returncode,
        "payload": payload,
        "stderr": stderr.strip(),
    }


def run_tool(cmd, args, timeout=None):
    effective_timeout = ARGS.tool_timeout if timeout is None else timeout
    argv = [
        BIN,
        PROJECT,
        "--preset",
        ARGS.preset,
        "--cmd",
        cmd,
        "--args",
        json.dumps(args),
    ]
    t0 = time.perf_counter()
    process = None
    try:
        process = subprocess.Popen(
            argv,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            env=RUN_ENV,
            start_new_session=True,
        )
        stdout, stderr = process.communicate(timeout=effective_timeout)
    except subprocess.TimeoutExpired as error:
        if process is not None:
            if hasattr(os, "killpg"):
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    process.poll()
            else:
                process.kill()
            try:
                stdout, stderr = process.communicate(timeout=1)
            except subprocess.TimeoutExpired:
                stdout = error.stdout
                stderr = error.stderr
        elapsed_ms = round((time.perf_counter() - t0) * 1000, 2)
        output = subprocess_text(stdout).strip()
        response_bytes = len(output.encode("utf-8"))
        return {
            "elapsed_ms": elapsed_ms,
            "response_bytes": response_bytes,
            "estimated_response_tokens": max(1, response_bytes // 4),
            "returncode": 124,
            "payload": {
                "success": False,
                "error": "tool_timeout",
                "tool": cmd,
                "timeout_seconds": effective_timeout,
            },
            "stderr": subprocess_text(stderr).strip(),
        }
    elapsed_ms = round((time.perf_counter() - t0) * 1000, 2)
    output = stdout.strip()
    payload = parse_output_json(output)
    return build_tool_result(
        elapsed_ms, process.returncode, payload, stderr, raw_output=output
    )


def returncode_for_batch_payload(process_returncode, payload):
    if tool_payload_succeeded(payload):
        return 0
    return process_returncode if process_returncode else 1


def build_batch_tool_result(
    batch_elapsed_ms,
    batch_amortized_elapsed_ms,
    returncode,
    payload,
    stderr,
    raw_output="",
):
    result = build_tool_result(
        batch_elapsed_ms,
        returncode,
        payload,
        stderr,
        raw_output=raw_output,
    )
    result["elapsed_ms"] = None
    result["batch_elapsed_ms"] = batch_elapsed_ms
    result["batch_amortized_elapsed_ms"] = batch_amortized_elapsed_ms
    return result


def run_tool_batch(cmd, args_list, timeout=None):
    if not args_list:
        return []
    per_call_timeout = ARGS.tool_timeout if timeout is None else timeout
    effective_timeout = per_call_timeout * len(args_list)
    batch = [{"name": cmd, "arguments": args} for args in args_list]
    argv = [
        BIN,
        PROJECT,
        "--preset",
        ARGS.preset,
        "--batch",
        json.dumps(batch),
    ]
    t0 = time.perf_counter()
    process = None
    try:
        process = subprocess.Popen(
            argv,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            env=RUN_ENV,
            start_new_session=True,
        )
        stdout, stderr = process.communicate(timeout=effective_timeout)
    except subprocess.TimeoutExpired as error:
        if process is not None:
            if hasattr(os, "killpg"):
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    process.poll()
            else:
                process.kill()
            try:
                stdout, stderr = process.communicate(timeout=1)
            except subprocess.TimeoutExpired:
                stdout = error.stdout
                stderr = error.stderr
        elapsed_ms = round((time.perf_counter() - t0) * 1000, 2)
        output = subprocess_text(stdout).strip()
        response_bytes = len(output.encode("utf-8"))
        return [
            {
                "elapsed_ms": elapsed_ms,
                "response_bytes": response_bytes,
                "estimated_response_tokens": max(1, response_bytes // 4),
                "returncode": 124,
                "payload": {
                    "success": False,
                    "error": "tool_timeout",
                    "tool": cmd,
                    "timeout_seconds": effective_timeout,
                },
                "stderr": subprocess_text(stderr).strip(),
            }
            for _ in args_list
        ]
    elapsed_ms = round((time.perf_counter() - t0) * 1000, 2)
    output = stdout.strip()
    payloads = parse_output_json(output)
    if not isinstance(payloads, list) or len(payloads) != len(args_list):
        payload = {
            "success": False,
            "error": "invalid_batch_output",
            "tool": cmd,
            "batch_size": len(args_list),
            "payload_type": type(payloads).__name__,
        }
        return [
            build_batch_tool_result(
                elapsed_ms,
                elapsed_ms,
                process.returncode if process.returncode else 1,
                payload,
                stderr,
                raw_output=output,
            )
            for _ in args_list
        ]

    per_call_elapsed_ms = round(elapsed_ms / len(args_list), 2)
    return [
        build_batch_tool_result(
            elapsed_ms,
            per_call_elapsed_ms,
            returncode_for_batch_payload(process.returncode, payload),
            payload,
            stderr,
            raw_output=output,
        )
        for payload in payloads
    ]


def tool_succeeded(result):
    payload = result.get("payload")
    return result.get("returncode") == 0 and tool_payload_succeeded(payload)


def require_tool_success(name, result, context=""):
    if tool_succeeded(result):
        return result
    message = [f"{name} failed"]
    if context:
        message.append(f"context={context}")
    message.append(f"returncode={result.get('returncode')}")
    payload = result.get("payload")
    if payload is not None:
        message.append(f"payload={json.dumps(payload, ensure_ascii=False)}")
    stderr = result.get("stderr")
    if stderr:
        message.append(f"stderr={stderr}")
    raise SystemExit(" | ".join(message))


def render_stdout_summary(result):
    methods = {method["method"]: method for method in result["methods"]}
    hybrid = methods["get_ranked_context"]
    diagnostics = result["ranker_diagnostics"]
    missing_rate = hybrid_candidate_missing_rate(diagnostics)
    demoted_hits = hybrid_demoted_semantic_hits(diagnostics)
    cache_probe = result.get("query_cache_probe")
    timings = result.get("timings") or {}
    lines = [
        "Embedding-quality summary:",
        f"dataset_size={result['dataset_size']}",
        f"methods={','.join(result.get('requested_methods') or [])}",
        f"workers={result['worker_count']}",
        f"method_workers={result['method_worker_count']}",
        f"batch_size={result['batch_size']}",
        f"total_elapsed_ms={format_optional_ms(timings.get('total_elapsed_ms'))}",
        "index_embeddings_elapsed_ms="
        f"{format_optional_ms(timings.get('index_embeddings_elapsed_ms'))}",
        f"hybrid_mrr={hybrid['mrr']:.6f}",
        f"hybrid_recall={hybrid['recall_at_cutoff']:.6f}",
        f"hybrid_acc1={hybrid['acc1']:.6f}",
        f"hybrid_avg_tokens={hybrid['avg_estimated_response_tokens']:.2f}",
        f"hybrid_p95_tokens={hybrid['p95_estimated_response_tokens']}",
        "lexical_mrr="
        + (
            f"{methods['get_ranked_context_no_semantic']['mrr']:.6f}"
            if "get_ranked_context_no_semantic" in methods
            else "skipped"
        ),
        "semantic_mrr="
        + (
            f"{methods['semantic_search']['mrr']:.6f}"
            if "semantic_search" in methods
            else "skipped"
        ),
        f"candidate_missing_rate={missing_rate:.6f}",
        f"hybrid_demoted_semantic_hits={demoted_hits}",
        "query_cache="
        + (
            f"{cache_probe.get('first_cache_hit_tier')}->{cache_probe.get('second_cache_hit_tier')}"
            if cache_probe
            else "skipped"
        ),
    ]
    if ARGS.output:
        lines.append(f"json_output={ARGS.output}")
    if ARGS.markdown_output:
        lines.append(f"markdown_output={ARGS.markdown_output}")
    if ARGS.triage_output:
        lines.append(f"triage_output={ARGS.triage_output}")
    return "\n".join(lines)


_IGNORE_PATTERNS = shutil.ignore_patterns(
    ".git",
    ".codelens",
    "target",
    "node_modules",
    ".next",
    "dist",
    "coverage",
    "__pycache__",
    ".venv",
    "venv",
    ".pytest_cache",
)


def copy_project_for_benchmark(source_project: str) -> str:
    # Deterministic copy: walks directory entries in sorted order so that the
    # indexer observes the same filesystem insertion order on every run.
    # shutil.copytree backs onto os.scandir whose order is filesystem-defined;
    # that ±0.003 MRR wobble is observable and we remove it here.
    source = Path(source_project).resolve()
    temp_root = Path(tempfile.mkdtemp(prefix="codelens-embed-quality-"))
    bench_project = temp_root / source.name

    def _copy_dir(src: Path, dst: Path) -> None:
        dst.mkdir(parents=True, exist_ok=True)
        entries = sorted(src.iterdir(), key=lambda p: p.name)
        ignored = _IGNORE_PATTERNS(str(src), [e.name for e in entries])
        for entry in entries:
            if entry.name in ignored:
                continue
            target = dst / entry.name
            if entry.is_symlink():
                link_target = os.readlink(entry)
                os.symlink(link_target, target)
            elif entry.is_dir():
                _copy_dir(entry, target)
            else:
                shutil.copy2(entry, target, follow_symlinks=False)

    _copy_dir(source, bench_project)
    # RBAC (ADR-0009): the deterministic copy excludes .codelens entirely,
    # which also drops principals.toml — the mutation-capable bench runtime
    # then resolves every principal to ReadOnly and index_embeddings is
    # denied. Stage a minimal Refactor-default mapping for the scratch copy.
    bench_codelens = bench_project / ".codelens"
    bench_codelens.mkdir(parents=True, exist_ok=True)
    (bench_codelens / "principals.toml").write_text(
        '[default]\nrole = "Refactor"\n', encoding="utf-8"
    )
    return str(bench_project)


def load_dataset():
    dataset = json.loads(Path(DATASET).read_text(encoding="utf-8"))
    validate_expected_file_suffixes(
        dataset,
        DATASET,
        lambda _row: PROJECT,
        row_label=lambda row: row.get("query") or row.get("expected_symbol"),
    )
    return dataset


def query_type_for_item(item):
    return item.get("query_type", "uncategorized")


def candidate_rows(method_name, payload):
    payload = payload or {}
    data = payload.get("data") if isinstance(payload.get("data"), dict) else payload
    if method_name == "semantic_search":
        return [
            {"name": row.get("symbol_name"), "file": row.get("file_path")}
            for row in data.get("results", [])
        ]
    if method_name in {"get_ranked_context", "get_ranked_context_no_semantic"}:
        return [
            {"name": row.get("name"), "file": row.get("file")}
            for row in data.get("symbols", [])
        ]
    if method_name == "bm25_symbol_search":
        return [
            {"name": row.get("name"), "file": row.get("file_path")}
            for row in data.get("results", [])
        ]
    return []


def find_rank(expected_symbol, expected_file_suffix, rows):
    for index, row in enumerate(rows, start=1):
        if row.get("name") != expected_symbol:
            continue
        if expected_file_suffix and not str(row.get("file", "")).endswith(
            expected_file_suffix
        ):
            continue
        return index
    return None


def mrr_component(rank):
    return 0.0 if rank is None else 1.0 / rank


def acc_at(rank, k):
    return 0.0 if rank is None else float(rank <= k)


def recall_at(rank, k):
    return acc_at(rank, k)


def percentile_value(values, percentile):
    if not values:
        return 0.0
    ordered = sorted(values)
    index = math.ceil(len(ordered) * percentile / 100) - 1
    index = min(max(index, 0), len(ordered) - 1)
    return ordered[index]


def optional_percentile_value(values, percentile):
    return percentile_value(values, percentile) if values else None


def format_optional_ms(value):
    return "n/a" if value is None else f"{value:.1f}"


def elapsed_ms_since(started_at):
    return round((time.perf_counter() - started_at) * 1000, 2)


def cache_hit_tier_from_payload(payload):
    data = payload_data(payload or {}) or {}
    query_cache = data.get("query_cache")
    retrieval = data.get("retrieval")
    candidates = [
        data.get("cache_hit_tier"),
        query_cache.get("cache_hit_tier") if isinstance(query_cache, dict) else None,
        retrieval.get("cache_hit_tier") if isinstance(retrieval, dict) else None,
    ]
    for candidate in candidates:
        if isinstance(candidate, str) and candidate:
            return candidate
    return None


def cache_hit_observed(tier):
    if tier is None:
        return False
    return tier.lower() in {"exact", "warm", "cachedexact", "cachedwarm"}


def ranked_context_args(item, disable_semantic=False):
    args = {
        "query": item["query"],
        "max_tokens": ARGS.ranked_context_max_tokens,
        "include_body": False,
    }
    if disable_semantic:
        args["disable_semantic"] = True
    return args


def method_metrics(rows):
    total = len(rows)
    elapsed_values = [
        row["elapsed_ms"] for row in rows if row.get("elapsed_ms") is not None
    ]
    amortized_values = [
        row["batch_amortized_elapsed_ms"]
        for row in rows
        if row.get("batch_amortized_elapsed_ms") is not None
    ]
    return {
        "mrr": sum(mrr_component(row["rank"]) for row in rows) / total,
        "acc1": sum(acc_at(row["rank"], 1) for row in rows) / total,
        "acc3": sum(acc_at(row["rank"], 3) for row in rows) / total,
        "acc5": sum(acc_at(row["rank"], 5) for row in rows) / total,
        "recall_at_cutoff": sum(
            recall_at(row["rank"], ARGS.max_results) for row in rows
        )
        / total,
        "avg_elapsed_ms": (
            sum(elapsed_values) / len(elapsed_values) if elapsed_values else None
        ),
        "p95_elapsed_ms": optional_percentile_value(elapsed_values, 95),
        "avg_batch_amortized_elapsed_ms": (
            sum(amortized_values) / len(amortized_values)
            if amortized_values
            else None
        ),
        "p95_batch_amortized_elapsed_ms": optional_percentile_value(
            amortized_values, 95
        ),
        "avg_response_bytes": sum(row["response_bytes"] for row in rows) / total,
        "p95_response_bytes": percentile_value(
            [row["response_bytes"] for row in rows], 95
        ),
        "avg_estimated_response_tokens": sum(
            row["estimated_response_tokens"] for row in rows
        )
        / total,
        "p95_estimated_response_tokens": percentile_value(
            [row["estimated_response_tokens"] for row in rows], 95
        ),
    }


def row_from_tool_result(name, tool_name, item, tool_result):
    tool_result = require_tool_success(
        tool_name,
        tool_result,
        context=item["query"],
    )
    payload = tool_result.get("payload") or {}
    candidates = candidate_rows(name, payload)
    rank = find_rank(
        item["expected_symbol"], item.get("expected_file_suffix"), candidates
    )
    return {
        "query": item["query"],
        "query_type": query_type_for_item(item),
        "expected_symbol": item["expected_symbol"],
        "expected_file_suffix": item.get("expected_file_suffix"),
        "rank": rank,
        "elapsed_ms": tool_result["elapsed_ms"],
        "batch_elapsed_ms": tool_result.get("batch_elapsed_ms"),
        "batch_amortized_elapsed_ms": tool_result.get(
            "batch_amortized_elapsed_ms"
        ),
        "response_bytes": tool_result["response_bytes"],
        "estimated_response_tokens": tool_result["estimated_response_tokens"],
        "cache_hit_tier": cache_hit_tier_from_payload(payload),
        "candidate_count": len(candidates),
        "top_candidate": candidates[0] if candidates else None,
    }


def chunked_rows(dataset):
    return [
        dataset[index : index + ARGS.batch_size]
        for index in range(0, len(dataset), ARGS.batch_size)
    ]


def evaluate_method(name, dataset, tool_name, args_factory):
    method_started = time.perf_counter()

    def evaluate_item(item):
        return row_from_tool_result(
            name,
            tool_name,
            item,
            run_tool(tool_name, args_factory(item)),
        )

    def evaluate_batch(batch):
        args_list = [args_factory(item) for item in batch]
        tool_results = run_tool_batch(tool_name, args_list)
        return [
            row_from_tool_result(name, tool_name, item, tool_result)
            for item, tool_result in zip(batch, tool_results, strict=True)
        ]

    if ARGS.batch_size == 1 and (ARGS.workers == 1 or len(dataset) <= 1):
        rows = [evaluate_item(item) for item in dataset]
    elif ARGS.batch_size == 1:
        with concurrent.futures.ThreadPoolExecutor(
            max_workers=ARGS.workers
        ) as executor:
            rows = list(executor.map(evaluate_item, dataset))
    elif ARGS.workers == 1:
        rows = [
            row
            for batch_result in map(evaluate_batch, chunked_rows(dataset))
            for row in batch_result
        ]
    else:
        batches = chunked_rows(dataset)
        with concurrent.futures.ThreadPoolExecutor(
            max_workers=ARGS.workers
        ) as executor:
            rows = [
                row
                for batch_result in executor.map(evaluate_batch, batches)
                for row in batch_result
            ]

    by_type = {}
    grouped = collections.defaultdict(list)
    for row in rows:
        grouped[row["query_type"]].append(row)
    for query_type, group in sorted(grouped.items()):
        by_type[query_type] = {"count": len(group), **method_metrics(group)}

    return {
        "method": name,
        "method_wall_ms": elapsed_ms_since(method_started),
        "subprocess_invocation_count": (
            len(dataset) if ARGS.batch_size == 1 else len(chunked_rows(dataset))
        ),
        **method_metrics(rows),
        "by_query_type": by_type,
        "rows": rows,
    }


def evaluate_method_specs(method_specs):
    def evaluate_spec(spec):
        return evaluate_method(*spec)

    if ARGS.method_workers == 1 or len(method_specs) <= 1:
        return [evaluate_spec(spec) for spec in method_specs]

    with concurrent.futures.ThreadPoolExecutor(
        max_workers=min(ARGS.method_workers, len(method_specs))
    ) as executor:
        futures = [executor.submit(evaluate_spec, spec) for spec in method_specs]
        return [future.result() for future in futures]


def measure_query_cache_probe(dataset):
    if not dataset:
        return None
    probe_started = time.perf_counter()
    item = dataset[0]
    first = require_tool_success(
        "get_ranked_context",
        run_tool("get_ranked_context", ranked_context_args(item)),
        context=f"{item['query']} cache probe first run",
    )
    second = require_tool_success(
        "get_ranked_context",
        run_tool("get_ranked_context", ranked_context_args(item)),
        context=f"{item['query']} cache probe second run",
    )
    first_tier = cache_hit_tier_from_payload(first.get("payload"))
    second_tier = cache_hit_tier_from_payload(second.get("payload"))
    return {
        "tool": "get_ranked_context",
        "query": item["query"],
        "first_elapsed_ms": first["elapsed_ms"],
        "second_elapsed_ms": second["elapsed_ms"],
        "latency_delta_ms": second["elapsed_ms"] - first["elapsed_ms"],
        "first_cache_hit_tier": first_tier,
        "second_cache_hit_tier": second_tier,
        "cache_hit_signal_available": first_tier is not None or second_tier is not None,
        "cache_hit_observed": cache_hit_observed(second_tier),
        "probe_elapsed_ms": elapsed_ms_since(probe_started),
    }


def row_key(row):
    return (
        row["query"],
        row["expected_symbol"],
        row.get("expected_file_suffix") or "",
    )


def candidate_name(candidate):
    if isinstance(candidate, dict):
        return candidate.get("name")
    return None


def ranker_diagnostic_cause_candidates(row):
    status = row.get("status")
    causes = []
    match status:
        case "candidate_missing":
            comparisons = row.get("comparison_methods_available") or {}
            if comparisons.get("semantic_search"):
                causes.append("expected_symbol_absent_from_semantic_and_hybrid_candidates")
            else:
                causes.append("expected_symbol_absent_from_hybrid_candidates")
        case "hybrid_candidate_missing":
            causes.append("expected_symbol_absent_from_hybrid_candidates")
        case "semantic_hit_dropped_by_hybrid":
            causes.append("semantic_candidate_not_preserved_by_hybrid")
            if row.get("lexical_rank") is None:
                causes.append("lexical_lane_also_missed_expected_symbol")
        case "hybrid_demoted_semantic_hit":
            causes.append("hybrid_rank_lower_than_semantic_rank")
            lexical_rank = row.get("lexical_rank")
            semantic_rank = row.get("semantic_rank")
            if lexical_rank is not None and semantic_rank is not None:
                if lexical_rank <= semantic_rank:
                    causes.append("lexical_lane_outranked_semantic_hit")
            if candidate_name(row.get("hybrid_top_candidate")) == candidate_name(
                row.get("lexical_top_candidate")
            ):
                causes.append("hybrid_top_candidate_matches_lexical_top_candidate")
        case _:
            pass
    if not causes and status not in {
        "hybrid_hit",
        "rank_preserved",
        "hybrid_improved_semantic_hit",
    }:
        causes.append("ranker_status_requires_manual_review")
    return causes


def ranker_diagnostics(methods):
    by_method = {
        method["method"]: {row_key(row): row for row in method["rows"]}
        for method in methods
    }
    semantic_rows = by_method.get("semantic_search", {})
    hybrid_rows = by_method.get("get_ranked_context", {})
    lexical_rows = by_method.get("get_ranked_context_no_semantic", {})
    comparison_methods_available = {
        "semantic_search": "semantic_search" in by_method,
        "get_ranked_context_no_semantic": "get_ranked_context_no_semantic"
        in by_method,
    }
    rows = []
    grouped = collections.defaultdict(collections.Counter)
    for key, hybrid in hybrid_rows.items():
        semantic = semantic_rows.get(key)
        lexical = lexical_rows.get(key)
        semantic_rank = semantic["rank"] if semantic else None
        lexical_rank = lexical["rank"] if lexical else None
        hybrid_rank = hybrid["rank"]
        semantic_comparison_available = comparison_methods_available["semantic_search"]
        if not semantic_comparison_available and hybrid_rank is None:
            status = "hybrid_candidate_missing"
        elif not semantic_comparison_available:
            status = "hybrid_hit"
        elif semantic_rank is None and hybrid_rank is None:
            status = "candidate_missing"
        elif semantic_rank is not None and hybrid_rank is None:
            status = "semantic_hit_dropped_by_hybrid"
        elif semantic_rank is not None and hybrid_rank > semantic_rank:
            status = "hybrid_demoted_semantic_hit"
        elif semantic_rank is not None and hybrid_rank < semantic_rank:
            status = "hybrid_improved_semantic_hit"
        elif semantic_rank is None and hybrid_rank is not None:
            status = "hybrid_rescued_missing_semantic"
        else:
            status = "rank_preserved"
        query_type = hybrid["query_type"]
        grouped[query_type][status] += 1
        grouped["all"][status] += 1
        row = {
            "query": hybrid["query"],
            "query_type": query_type,
            "expected_symbol": hybrid["expected_symbol"],
            "semantic_rank": semantic_rank,
            "lexical_rank": lexical_rank,
            "hybrid_rank": hybrid_rank,
            "semantic_top_candidate": semantic["top_candidate"] if semantic else None,
            "lexical_top_candidate": lexical["top_candidate"] if lexical else None,
            "hybrid_top_candidate": hybrid["top_candidate"],
            "status": status,
            "comparison_methods_available": comparison_methods_available,
        }
        row["cause_candidates"] = ranker_diagnostic_cause_candidates(row)
        rows.append(row)
    return {
        "comparison_methods_available": comparison_methods_available,
        "by_query_type": {
            query_type: dict(counter) for query_type, counter in sorted(grouped.items())
        },
        "rows": rows,
    }


def parse_query_type_floors(raw_values, metric_label):
    floors = {}
    for raw in raw_values:
        if "=" not in raw:
            raise SystemExit(f"{metric_label} floor must use QUERY_TYPE=FLOOR: {raw}")
        query_type, value = raw.split("=", 1)
        query_type = query_type.strip()
        if not query_type:
            raise SystemExit(f"{metric_label} floor has an empty query type: {raw}")
        try:
            floor = float(value)
        except ValueError as exc:
            raise SystemExit(f"{metric_label} floor is not a number: {raw}") from exc
        if floor < 0.0 or floor > 1.0:
            raise SystemExit(f"{metric_label} floor must be between 0 and 1: {raw}")
        floors[query_type] = floor
    return floors


def add_query_type_floor_failures(failures, method, floors, metric_key, metric_label):
    for query_type, floor in floors.items():
        metrics = method.get("by_query_type", {}).get(query_type)
        if metrics is None:
            failures.append(f"hybrid query type missing for {metric_label}: {query_type}")
            continue
        value = metrics[metric_key]
        if value < floor:
            failures.append(
                f"hybrid {query_type} {metric_label} {value:.3f} < floor {floor:.3f}"
            )


def add_numeric_ceiling_failure(failures, method, metric_key, metric_label, ceiling):
    if ceiling <= 0:
        return
    value = method[metric_key]
    if value is None:
        failures.append(
            f"hybrid {metric_label} unavailable with batch_size > 1; rerun with --batch-size 1 for this latency gate"
        )
        return
    if value > ceiling:
        failures.append(
            f"hybrid {metric_label} {value:.0f} > ceiling {ceiling:.0f}"
        )


def hybrid_demoted_semantic_hits(diagnostics):
    counts = (diagnostics.get("by_query_type") or {}).get("all") or {}
    return counts.get("semantic_hit_dropped_by_hybrid", 0) + counts.get(
        "hybrid_demoted_semantic_hit", 0
    )


def hybrid_candidate_missing_rate(diagnostics):
    counts = (diagnostics.get("by_query_type") or {}).get("all") or {}
    total = sum(counts.values())
    if not total:
        return 0.0
    return (
        counts.get("candidate_missing", 0)
        + counts.get("hybrid_candidate_missing", 0)
    ) / total


def ranker_rows_by_status(diagnostics, status):
    return [
        row
        for row in diagnostics.get("rows", [])
        if row.get("status") == status
    ]


def ranker_rows_by_statuses(diagnostics, statuses):
    wanted = set(statuses)
    return [
        row
        for row in diagnostics.get("rows", [])
        if row.get("status") in wanted
    ]


def build_triage_artifact(result):
    diagnostics = result.get("ranker_diagnostics") or {}
    hybrid = next(
        method for method in result["methods"] if method["method"] == "get_ranked_context"
    )
    status_counts = (diagnostics.get("by_query_type") or {}).get("all") or {}
    candidate_missing_rows = ranker_rows_by_statuses(
        diagnostics, ["candidate_missing", "hybrid_candidate_missing"]
    )
    dropped_rows = ranker_rows_by_status(
        diagnostics, "semantic_hit_dropped_by_hybrid"
    )
    demoted_rows = ranker_rows_by_status(
        diagnostics, "hybrid_demoted_semantic_hit"
    )
    total = sum(status_counts.values())
    return {
        "schema_version": 1,
        "project": result["project"],
        "benchmark_project": result["benchmark_project"],
        "binary": result["binary"],
        "dataset_path": result["dataset_path"],
        "dataset_size": result["dataset_size"],
        "ranking_cutoff": result["ranking_cutoff"],
        "requested_methods": result.get("requested_methods") or [],
        "worker_count": result["worker_count"],
        "method_worker_count": result["method_worker_count"],
        "batch_size": result["batch_size"],
        "query_cache_probe_enabled": result["query_cache_probe_enabled"],
        "timings": result.get("timings") or {},
        "hybrid_metrics": {
            "mrr": hybrid["mrr"],
            "recall_at_cutoff": hybrid["recall_at_cutoff"],
            "acc1": hybrid["acc1"],
            "method_wall_ms": hybrid["method_wall_ms"],
            "subprocess_invocation_count": hybrid["subprocess_invocation_count"],
            "avg_elapsed_ms": hybrid["avg_elapsed_ms"],
            "p95_elapsed_ms": hybrid["p95_elapsed_ms"],
            "avg_batch_amortized_elapsed_ms": hybrid[
                "avg_batch_amortized_elapsed_ms"
            ],
            "p95_batch_amortized_elapsed_ms": hybrid[
                "p95_batch_amortized_elapsed_ms"
            ],
            "avg_estimated_response_tokens": hybrid[
                "avg_estimated_response_tokens"
            ],
            "p95_estimated_response_tokens": hybrid[
                "p95_estimated_response_tokens"
            ],
            "avg_response_bytes": hybrid["avg_response_bytes"],
            "p95_response_bytes": hybrid["p95_response_bytes"],
        },
        "status_counts": status_counts,
        "status_counts_by_query_type": diagnostics.get("by_query_type") or {},
        "comparison_methods_available": diagnostics.get(
            "comparison_methods_available"
        )
        or {},
        "candidate_missing": {
            "count": len(candidate_missing_rows),
            "rate": (len(candidate_missing_rows) / total) if total else 0.0,
            "rows": candidate_missing_rows,
        },
        "semantic_hit_dropped_by_hybrid": {
            "count": len(dropped_rows),
            "rows": dropped_rows,
        },
        "hybrid_demoted_semantic_hit": {
            "count": len(demoted_rows),
            "rows": demoted_rows,
        },
        "token_budget": {
            "avg_response_tokens": hybrid["avg_estimated_response_tokens"],
            "p95_response_tokens": hybrid["p95_estimated_response_tokens"],
            "avg_response_bytes": hybrid["avg_response_bytes"],
            "p95_response_bytes": hybrid["p95_response_bytes"],
        },
        "query_cache_probe": result.get("query_cache_probe"),
    }


def render_markdown(result):
    lines = []
    a = lines.append
    runtime_model = result.get("runtime_model") or {}
    a("# Embedding Quality Summary")
    a("")
    a(f"- Project: `{result['project']}`")
    a(f"- Binary: `{result['binary']}`")
    a(f"- Embedding model: `{result['embedding_model']}`")
    if result.get("requested_embed_model"):
        a(f"- Requested embed model override: `{result['requested_embed_model']}`")
    if runtime_model:
        a(
            f"- Runtime backend: `{runtime_model.get('backend')}`, "
            f"preference=`{runtime_model.get('runtime_preference')}`, "
            f"max_length=`{runtime_model.get('max_length')}`"
        )
        if runtime_model.get("model_path"):
            a(
                f"- Runtime model: `{runtime_model.get('num_hidden_layers', '?')}L`, "
                f"`{runtime_model.get('size_bytes', 0) // (1024 * 1024)}MB`, "
                f"`sha256:{str(runtime_model.get('sha256', ''))[:16]}`"
            )
            a(f"- Runtime model path: `{runtime_model.get('model_path')}`")
    a(f"- Dataset size: {result['dataset_size']}")
    a(f"- Ranking cutoff: top-{result['ranking_cutoff']}")
    if result.get("requested_methods"):
        a(f"- Requested methods: `{', '.join(result['requested_methods'])}`")
    a(f"- Workers: {result['worker_count']}")
    a(f"- Method workers: {result['method_worker_count']}")
    a(f"- Batch size: {result['batch_size']}")
    a(
        "- Query cache probe: "
        + ("enabled" if result["query_cache_probe_enabled"] else "skipped")
    )
    timings = result.get("timings") or {}
    if timings:
        a("")
        a("## Timings")
        a("")
        a("| Phase | Wall ms |")
        a("|---|---:|")
        a(f"| total | {format_optional_ms(timings.get('total_elapsed_ms'))} |")
        a(
            "| dataset_load | "
            f"{format_optional_ms(timings.get('dataset_load_elapsed_ms'))} |"
        )
        a(
            "| get_capabilities | "
            f"{format_optional_ms(timings.get('get_capabilities_elapsed_ms'))} |"
        )
        a(
            "| index_embeddings | "
            f"{format_optional_ms(timings.get('index_embeddings_elapsed_ms'))} |"
        )
        a(
            "| query_cache_probe | "
            f"{format_optional_ms(timings.get('query_cache_probe_elapsed_ms'))} |"
        )
    a("")
    a("## Metrics")
    a("")
    a(
        f"| Method | MRR@{result['ranking_cutoff']} | Recall@{result['ranking_cutoff']} | Acc@1 | Acc@3 | Acc@5 | Method wall ms | Calls | Avg ms | P95 ms | Avg batch ms | P95 batch ms | Avg bytes | P95 bytes | Avg tokens | P95 tokens |"
    )
    a("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
    for method in result["methods"]:
        a(
            f"| {method['method']} | {method['mrr']:.3f} | {method['recall_at_cutoff']:.0%} | {method['acc1']:.0%} | {method['acc3']:.0%} | {method['acc5']:.0%} | {format_optional_ms(method['method_wall_ms'])} | {method['subprocess_invocation_count']} | {format_optional_ms(method['avg_elapsed_ms'])} | {format_optional_ms(method['p95_elapsed_ms'])} | {format_optional_ms(method['avg_batch_amortized_elapsed_ms'])} | {format_optional_ms(method['p95_batch_amortized_elapsed_ms'])} | {method['avg_response_bytes']:.0f} | {method['p95_response_bytes']:.0f} | {method['avg_estimated_response_tokens']:.0f} | {method['p95_estimated_response_tokens']:.0f} |"
        )
    a("")
    a("## Query Type Breakdown")
    a("")
    a(
        "| Method | Query type | Count | MRR | Recall | Acc@1 | Acc@3 | Acc@5 | Avg ms | P95 ms | Avg batch ms | P95 batch ms | Avg bytes | P95 bytes | Avg tokens | P95 tokens |"
    )
    a("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
    for method in result["methods"]:
        for query_type, metrics in method.get("by_query_type", {}).items():
            a(
                f"| {method['method']} | {query_type} | {metrics['count']} | {metrics['mrr']:.3f} | {metrics['recall_at_cutoff']:.0%} | {metrics['acc1']:.0%} | {metrics['acc3']:.0%} | {metrics['acc5']:.0%} | {format_optional_ms(metrics['avg_elapsed_ms'])} | {format_optional_ms(metrics['p95_elapsed_ms'])} | {format_optional_ms(metrics['avg_batch_amortized_elapsed_ms'])} | {format_optional_ms(metrics['p95_batch_amortized_elapsed_ms'])} | {metrics['avg_response_bytes']:.0f} | {metrics['p95_response_bytes']:.0f} | {metrics['avg_estimated_response_tokens']:.0f} | {metrics['p95_estimated_response_tokens']:.0f} |"
            )
    cache_probe = result.get("query_cache_probe") or {}
    if cache_probe:
        a("")
        a("## Query Cache Probe")
        a("")
        a("| Query | First ms | Second ms | First tier | Second tier | Hit observed |")
        a("|---|---:|---:|---|---|---|")
        a(
            f"| {cache_probe['query']} | {cache_probe['first_elapsed_ms']:.1f} | {cache_probe['second_elapsed_ms']:.1f} | {cache_probe.get('first_cache_hit_tier') or 'none'} | {cache_probe.get('second_cache_hit_tier') or 'none'} | {cache_probe['cache_hit_observed']} |"
        )
    uplift = result.get("hybrid_uplift")
    if uplift:
        a("")
        a("## Hybrid Uplift")
        a("")
        a("| KPI | Delta |")
        a("|---|---:|")
        a(f"| MRR uplift | {uplift['mrr_delta']:+.3f} |")
        a(f"| Acc@1 uplift | {uplift['acc1_delta']:+.0%} |")
        a(f"| Acc@3 uplift | {uplift['acc3_delta']:+.0%} |")
        a(f"| Acc@5 uplift | {uplift['acc5_delta']:+.0%} |")
    if result.get("hybrid_uplift_by_query_type"):
        a("")
        a("## Hybrid Uplift by Query Type")
        a("")
        a("| Query type | MRR | Acc@1 | Acc@3 | Acc@5 |")
        a("|---|---:|---:|---:|---:|")
        for query_type, metrics in sorted(
            result["hybrid_uplift_by_query_type"].items()
        ):
            a(
                f"| {query_type} | {metrics['mrr_delta']:+.3f} | {metrics['acc1_delta']:+.0%} | {metrics['acc3_delta']:+.0%} | {metrics['acc5_delta']:+.0%} |"
            )
    diagnostics = result.get("ranker_diagnostics") or {}
    if diagnostics.get("by_query_type"):
        a("")
        a("## Ranker Diagnostics")
        a("")
        a("| Query type | Status | Count |")
        a("|---|---|---:|")
        for query_type, counts in diagnostics["by_query_type"].items():
            for status, count in sorted(counts.items()):
                a(f"| {query_type} | {status} | {count} |")
        detailed_rows = [
            row
            for row in diagnostics.get("rows", [])
            if row.get("status")
            in {
                "candidate_missing",
                "hybrid_candidate_missing",
                "semantic_hit_dropped_by_hybrid",
                "hybrid_demoted_semantic_hit",
            }
        ]
        if detailed_rows:
            a("")
            a("## Ranker Diagnostic Details")
            a("")
            a(
                "| Query type | Status | Query | Expected | Semantic rank | Hybrid rank | Hybrid top candidate | Cause candidates |"
            )
            a("|---|---|---|---|---:|---:|---|---|")
            for row in detailed_rows:
                top = row.get("hybrid_top_candidate") or {}
                top_label = ""
                if top:
                    top_label = f"{top.get('name')} ({top.get('file')})"
                causes = ", ".join(row.get("cause_candidates") or [])
                a(
                    f"| {row['query_type']} | {row['status']} | {row['query']} | {row['expected_symbol']} | {row['semantic_rank'] if row['semantic_rank'] is not None else 'miss'} | {row['hybrid_rank'] if row['hybrid_rank'] is not None else 'miss'} | {top_label} | {causes} |"
                )
    a("")
    a("## Misses")
    a("")
    a("| Method | Query | Rank | Top candidate |")
    a("|---|---|---:|---|")
    for method in result["methods"]:
        for row in method["rows"]:
            if row["rank"] is not None and row["rank"] <= 3:
                continue
            top = row["top_candidate"]
            top_label = ""
            if top:
                top_label = f"{top.get('name')} ({top.get('file')})"
            a(
                f"| {method['method']} | {row['query']} | {row['rank'] if row['rank'] is not None else 'miss'} | {top_label} |"
            )
    a("")
    return "\n".join(lines)


def main():
    benchmark_started = time.perf_counter()
    mrr_type_floors = parse_query_type_floors(
        ARGS.min_hybrid_mrr_by_query_type, "MRR"
    )
    recall_type_floors = parse_query_type_floors(
        ARGS.min_hybrid_recall_by_query_type, f"Recall@{ARGS.max_results}"
    )
    acc1_type_floors = parse_query_type_floors(
        ARGS.min_hybrid_acc1_by_query_type, "Acc@1"
    )
    dataset_started = time.perf_counter()
    dataset = load_dataset()
    dataset_load_elapsed_ms = elapsed_ms_since(dataset_started)
    capabilities = require_tool_success(
        "get_capabilities", run_tool("get_capabilities", {})
    )
    capability_data = payload_data(capabilities.get("payload") or {}) or {}
    embedding_model = capability_data.get("embedding_model")
    runtime_model = snapshot_runtime_model(capabilities.get("payload") or {})

    index_embeddings = require_tool_success(
        # background=false: the one-shot CLI process exits after replying, so a
        # queued background indexing job would be killed before it embeds
        # anything and every later query sees an empty index.
        "index_embeddings",
        run_tool("index_embeddings", {"background": False}, timeout=1800),
    )

    requested_methods = parse_requested_methods(ARGS.methods)
    method_specs = []
    if "semantic_search" in requested_methods:
        method_specs.append(
            (
                "semantic_search",
                dataset,
                "semantic_search",
                lambda item: {"query": item["query"], "max_results": ARGS.max_results},
            )
        )
    if "get_ranked_context_no_semantic" in requested_methods:
        method_specs.append(
            (
                "get_ranked_context_no_semantic",
                dataset,
                "get_ranked_context",
                lambda item: ranked_context_args(item, disable_semantic=True),
            )
        )
    if "get_ranked_context" in requested_methods:
        method_specs.append(
            (
                "get_ranked_context",
                dataset,
                "get_ranked_context",
                ranked_context_args,
            )
        )
    if "bm25_symbol_search" in requested_methods:
        method_specs.append(
            (
                "bm25_symbol_search",
                dataset,
                "bm25_symbol_search",
                lambda item: {
                    "query": item["query"],
                    "max_results": ARGS.max_results,
                },
            )
        )
    methods = evaluate_method_specs(method_specs)

    lexical = next(
        (
            method
            for method in methods
            if method["method"] == "get_ranked_context_no_semantic"
        ),
        None,
    )
    hybrid = next(
        method for method in methods if method["method"] == "get_ranked_context"
    )
    hybrid_uplift_by_query_type = {}
    if lexical is not None:
        for query_type, metrics in hybrid["by_query_type"].items():
            lexical_metrics = lexical["by_query_type"].get(query_type)
            if not lexical_metrics:
                continue
            hybrid_uplift_by_query_type[query_type] = {
                "mrr_delta": metrics["mrr"] - lexical_metrics["mrr"],
                "acc1_delta": metrics["acc1"] - lexical_metrics["acc1"],
                "acc3_delta": metrics["acc3"] - lexical_metrics["acc3"],
                "acc5_delta": metrics["acc5"] - lexical_metrics["acc5"],
            }

    query_cache_probe_enabled = ARGS.query_cache_probe == "on"
    query_cache_probe = (
        measure_query_cache_probe(dataset) if query_cache_probe_enabled else None
    )
    timings = {
        "dataset_load_elapsed_ms": dataset_load_elapsed_ms,
        "get_capabilities_elapsed_ms": capabilities["elapsed_ms"],
        "index_embeddings_elapsed_ms": index_embeddings["elapsed_ms"],
        "method_worker_count": ARGS.method_workers,
        "method_wall_ms": {
            method["method"]: method["method_wall_ms"] for method in methods
        },
        "method_subprocess_invocations": {
            method["method"]: method["subprocess_invocation_count"]
            for method in methods
        },
        "query_cache_probe_elapsed_ms": (
            query_cache_probe.get("probe_elapsed_ms") if query_cache_probe else None
        ),
    }
    result = {
        "project": SOURCE_PROJECT,
        "benchmark_project": PROJECT,
        "isolated_copy": bool(ARGS.isolated_copy),
        "binary": BIN,
        "requested_embed_model": ARGS.embed_model or None,
        "embedding_model": embedding_model,
        "runtime_model": runtime_model,
        "dataset_path": DATASET,
        "dataset_size": len(dataset),
        "ranking_cutoff": ARGS.max_results,
        "requested_methods": requested_methods,
        "worker_count": ARGS.workers,
        "method_worker_count": ARGS.method_workers,
        "batch_size": ARGS.batch_size,
        "query_cache_probe_enabled": query_cache_probe_enabled,
        "timings": timings,
        "metric_labels": {
            "mrr": f"MRR@{ARGS.max_results}",
            "recall_at_cutoff": f"Recall@{ARGS.max_results}",
            "acc1": "Acc@1",
            "acc3": "Acc@3",
            "acc5": "Acc@5",
            "avg_elapsed_ms": "Avg latency ms",
            "p95_elapsed_ms": "P95 latency ms",
            "avg_batch_amortized_elapsed_ms": "Avg batch-amortized latency ms",
            "p95_batch_amortized_elapsed_ms": "P95 batch-amortized latency ms",
            "avg_response_bytes": "Avg compact JSON payload bytes",
            "p95_response_bytes": "P95 compact JSON payload bytes",
            "avg_estimated_response_tokens": "Avg estimated response tokens",
            "p95_estimated_response_tokens": "P95 estimated response tokens",
        },
        "methods": methods,
        "hybrid_uplift": (
            {
                "mrr_delta": hybrid["mrr"] - lexical["mrr"],
                "acc1_delta": hybrid["acc1"] - lexical["acc1"],
                "acc3_delta": hybrid["acc3"] - lexical["acc3"],
                "acc5_delta": hybrid["acc5"] - lexical["acc5"],
            }
            if lexical is not None
            else None
        ),
        "hybrid_uplift_by_query_type": hybrid_uplift_by_query_type,
        "ranker_diagnostics": ranker_diagnostics(methods),
        "query_cache_probe": query_cache_probe,
    }
    result["timings"]["total_elapsed_ms"] = elapsed_ms_since(benchmark_started)

    output_text = json.dumps(result, ensure_ascii=False, indent=2)
    match ARGS.stdout:
        case "json":
            print(output_text)
        case "summary":
            print(render_stdout_summary(result))
        case "none":
            pass
    if ARGS.output:
        Path(ARGS.output).write_text(output_text + "\n", encoding="utf-8")
    if ARGS.markdown_output:
        Path(ARGS.markdown_output).write_text(
            render_markdown(result) + "\n", encoding="utf-8"
        )
    if ARGS.triage_output:
        triage_text = json.dumps(
            build_triage_artifact(result), ensure_ascii=False, indent=2
        )
        Path(ARGS.triage_output).write_text(triage_text + "\n", encoding="utf-8")

    if ARGS.check:
        import sys

        failures = []
        if hybrid["mrr"] < ARGS.min_hybrid_mrr:
            failures.append(
                f"hybrid MRR {hybrid['mrr']:.3f} < floor {ARGS.min_hybrid_mrr:.3f}"
            )
        if lexical is not None and lexical["mrr"] < ARGS.min_lexical_mrr:
            failures.append(
                f"lexical MRR {lexical['mrr']:.3f} < floor {ARGS.min_lexical_mrr:.3f}"
            )
        if (
            ARGS.min_hybrid_recall > 0
            and hybrid["recall_at_cutoff"] < ARGS.min_hybrid_recall
        ):
            failures.append(
                f"hybrid Recall@{ARGS.max_results} {hybrid['recall_at_cutoff']:.3f} < floor {ARGS.min_hybrid_recall:.3f}"
            )
        if ARGS.min_hybrid_acc1 > 0 and hybrid["acc1"] < ARGS.min_hybrid_acc1:
            failures.append(
                f"hybrid Acc@1 {hybrid['acc1']:.3f} < floor {ARGS.min_hybrid_acc1:.3f}"
            )
        add_query_type_floor_failures(
            failures, hybrid, mrr_type_floors, "mrr", "MRR"
        )
        add_query_type_floor_failures(
            failures,
            hybrid,
            recall_type_floors,
            "recall_at_cutoff",
            f"Recall@{ARGS.max_results}",
        )
        add_query_type_floor_failures(
            failures, hybrid, acc1_type_floors, "acc1", "Acc@1"
        )
        if ARGS.max_hybrid_demoted_semantic_hits >= 0:
            demoted_hits = hybrid_demoted_semantic_hits(result["ranker_diagnostics"])
            if demoted_hits > ARGS.max_hybrid_demoted_semantic_hits:
                failures.append(
                    "hybrid demoted semantic hits "
                    f"{demoted_hits} > ceiling {ARGS.max_hybrid_demoted_semantic_hits}"
                )
        if ARGS.max_hybrid_candidate_missing_rate >= 0:
            missing_rate = hybrid_candidate_missing_rate(result["ranker_diagnostics"])
            if missing_rate > ARGS.max_hybrid_candidate_missing_rate:
                failures.append(
                    "hybrid candidate_missing rate "
                    f"{missing_rate:.3f} > ceiling {ARGS.max_hybrid_candidate_missing_rate:.3f}"
                )
        add_numeric_ceiling_failure(
            failures,
            hybrid,
            "avg_elapsed_ms",
            "avg latency ms",
            ARGS.max_hybrid_avg_ms,
        )
        add_numeric_ceiling_failure(
            failures,
            hybrid,
            "avg_response_bytes",
            "avg response bytes",
            ARGS.max_hybrid_avg_response_bytes,
        )
        add_numeric_ceiling_failure(
            failures,
            hybrid,
            "p95_estimated_response_tokens",
            "P95 estimated response tokens",
            ARGS.max_hybrid_p95_response_tokens,
        )
        if failures:
            print("\nEmbedding-quality gate failed:")
            for failure in failures:
                print(f"- {failure}")
            sys.exit(1)
        print(
            f"\nEmbedding-quality gate passed: "
            f"hybrid MRR {hybrid['mrr']:.3f} >= {ARGS.min_hybrid_mrr:.3f}"
            + (
                f", lexical MRR {lexical['mrr']:.3f} >= {ARGS.min_lexical_mrr:.3f}"
                if lexical is not None
                else ", lexical MRR skipped"
            )
        )


if __name__ == "__main__":
    cleanup_dir = None
    if ARGS.isolated_copy:
        PROJECT = copy_project_for_benchmark(SOURCE_PROJECT)
        cleanup_dir = str(Path(PROJECT).parent)
    try:
        main()
    finally:
        if cleanup_dir and not ARGS.keep_isolated_copy:
            shutil.rmtree(cleanup_dir, ignore_errors=True)
