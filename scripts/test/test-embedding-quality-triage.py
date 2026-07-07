#!/usr/bin/env -S uv run --script
# noqa: SIZE_OK - focused benchmark triage artifact contract tests stay together.
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

# --- How to run ---
# 1. Install uv (if not installed):
#      curl -LsSf https://astral.sh/uv/install.sh | sh
# 2. Run directly:
#      uv run scripts/test/test-embedding-quality-triage.py
# 3. CI can also run it with system Python:
#      python3 scripts/test/test-embedding-quality-triage.py
# ------------------

from __future__ import annotations

import importlib.util
import subprocess
import sys
from pathlib import Path
from types import ModuleType

REPO_ROOT = Path(__file__).resolve().parents[2]
BENCHMARKS_DIR = REPO_ROOT / "benchmarks"
BENCHMARK_SCRIPT = BENCHMARKS_DIR / "embedding-quality.py"


def load_embedding_quality_module() -> ModuleType:
    sys.path.insert(0, str(BENCHMARKS_DIR))
    old_argv = sys.argv[:]
    sys.argv = [str(BENCHMARK_SCRIPT)]
    try:
        spec = importlib.util.spec_from_file_location(
            "embedding_quality_benchmark", BENCHMARK_SCRIPT
        )
        assert spec is not None
        assert spec.loader is not None
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module
    finally:
        sys.argv = old_argv


def candidate(name: str, file_path: str) -> dict[str, str]:
    return {"name": name, "file": file_path}


def row(query: str, query_type: str, rank: int | None, top_name: str) -> dict[str, object]:
    top = candidate(top_name, f"src/{top_name}.rs") if top_name else None
    return {
        "query": query,
        "query_type": query_type,
        "expected_symbol": f"{query}_target",
        "expected_file_suffix": f"src/{query}_target.rs",
        "rank": rank,
        "top_candidate": top,
    }


def method(name: str, rows: list[dict[str, object]]) -> dict[str, object]:
    return {
        "method": name,
        "rows": rows,
        "method_wall_ms": 123.0,
        "subprocess_invocation_count": 2,
        "mrr": 0.7,
        "recall_at_cutoff": 0.8,
        "acc1": 0.6,
        "acc3": 0.7,
        "acc5": 0.8,
        "avg_elapsed_ms": 10.0,
        "p95_elapsed_ms": 20.0,
        "avg_batch_amortized_elapsed_ms": None,
        "p95_batch_amortized_elapsed_ms": None,
        "avg_estimated_response_tokens": 100,
        "p95_estimated_response_tokens": 250,
        "avg_response_bytes": 400,
        "p95_response_bytes": 800,
    }


def hybrid_method(methods: list[dict[str, object]]) -> dict[str, object]:
    return next(method for method in methods if method["method"] == "get_ranked_context")


def sample_methods() -> list[dict[str, object]]:
    semantic_rows = [
        row("missing", "natural_language", None, ""),
        row("dropped", "issue_to_edit", 1, "dropped_target"),
        row("demoted", "natural_language", 1, "demoted_target"),
    ]
    lexical_rows = [
        row("missing", "natural_language", None, ""),
        row("dropped", "issue_to_edit", 4, "lexical_noise"),
        row("demoted", "natural_language", 1, "lexical_noise"),
    ]
    hybrid_rows = [
        row("missing", "natural_language", None, ""),
        row("dropped", "issue_to_edit", None, ""),
        row("demoted", "natural_language", 3, "lexical_noise"),
    ]
    return [
        method("semantic_search", semantic_rows),
        method("get_ranked_context_no_semantic", lexical_rows),
        method("get_ranked_context", hybrid_rows),
    ]


def sample_result(
    module: ModuleType, query_cache_probe_enabled: bool = True
) -> dict[str, object]:
    methods = sample_methods()
    return {
        "project": "/repo",
        "benchmark_project": "/repo",
        "binary": "/repo/target/debug/codelens-mcp",
        "dataset_path": "/repo/benchmarks/embedding-quality-dataset-self.json",
        "dataset_size": 3,
        "ranking_cutoff": 10,
        "worker_count": 4,
        "method_worker_count": 3,
        "batch_size": 8,
        "timings": {
            "dataset_load_elapsed_ms": 1.0,
            "get_capabilities_elapsed_ms": 2.0,
            "index_embeddings_elapsed_ms": 3.0,
            "method_worker_count": 3,
            "method_wall_ms": {
                method["method"]: method["method_wall_ms"] for method in methods
            },
            "method_subprocess_invocations": {
                method["method"]: method["subprocess_invocation_count"]
                for method in methods
            },
            "query_cache_probe_elapsed_ms": 30.0 if query_cache_probe_enabled else None,
            "total_elapsed_ms": 456.0,
        },
        "embedding_model": "MiniLM-L12-CodeSearchNet-INT8",
        "requested_embed_model": None,
        "requested_methods": [method["method"] for method in methods],
        "query_cache_probe_enabled": query_cache_probe_enabled,
        "methods": methods,
        "hybrid_uplift": {
            "mrr_delta": 0.1,
            "acc1_delta": 0.1,
            "acc3_delta": 0.1,
            "acc5_delta": 0.1,
        },
        "hybrid_uplift_by_query_type": {},
        "ranker_diagnostics": module.ranker_diagnostics(methods),
        "query_cache_probe": (
            {
                "query": "demoted",
                "first_elapsed_ms": 20.0,
                "second_elapsed_ms": 10.0,
                "first_cache_hit_tier": None,
                "cache_hit_signal_available": True,
                "cache_hit_observed": True,
                "second_cache_hit_tier": "warm",
            }
            if query_cache_probe_enabled
            else None
        ),
    }


def assert_cause_candidates(rows: list[dict[str, object]]) -> None:
    assert rows
    for diagnostic_row in rows:
        causes = diagnostic_row.get("cause_candidates")
        assert isinstance(causes, list)
        assert causes
        assert all(isinstance(cause, str) and cause for cause in causes)


def test_ranker_diagnostics_records_cause_candidates() -> None:
    module = load_embedding_quality_module()

    diagnostics = module.ranker_diagnostics(sample_methods())
    hybrid_only = module.ranker_diagnostics([sample_methods()[2]])
    hybrid_only_missing = hybrid_only["rows"][0]

    assert hybrid_only_missing["status"] == "hybrid_candidate_missing"
    assert (
        "expected_symbol_absent_from_hybrid_candidates"
        in hybrid_only_missing["cause_candidates"]
    )
    assert_cause_candidates(
        [
            diagnostic_row
            for diagnostic_row in diagnostics["rows"]
            if diagnostic_row["status"]
            in {
                "candidate_missing",
                "semantic_hit_dropped_by_hybrid",
                "hybrid_demoted_semantic_hit",
            }
        ]
    )


def test_triage_artifact_preserves_diagnostic_cause_candidates() -> None:
    module = load_embedding_quality_module()

    artifact = module.build_triage_artifact(sample_result(module))

    assert artifact["requested_methods"] == [
        "semantic_search",
        "get_ranked_context_no_semantic",
        "get_ranked_context",
    ]
    assert artifact["worker_count"] == 4
    assert artifact["method_worker_count"] == 3
    assert artifact["batch_size"] == 8
    assert artifact["query_cache_probe_enabled"] is True
    assert artifact["timings"]["total_elapsed_ms"] == 456.0
    assert artifact["hybrid_metrics"]["method_wall_ms"] == 123.0
    assert artifact["hybrid_metrics"]["subprocess_invocation_count"] == 2
    assert artifact["token_budget"]["p95_response_tokens"] == 250
    assert artifact["query_cache_probe"]["cache_hit_observed"] is True
    assert_cause_candidates(artifact["candidate_missing"]["rows"])
    assert_cause_candidates(artifact["semantic_hit_dropped_by_hybrid"]["rows"])
    assert_cause_candidates(artifact["hybrid_demoted_semantic_hit"]["rows"])


def test_markdown_summary_renders_diagnostic_cause_candidates() -> None:
    module = load_embedding_quality_module()

    summary = module.render_markdown(sample_result(module))

    assert "Requested methods" in summary
    assert "get_ranked_context" in summary
    assert "Workers: 4" in summary
    assert "Method workers: 3" in summary
    assert "Batch size: 8" in summary
    assert "## Timings" in summary
    assert "Method wall ms" in summary
    assert "Query cache probe: enabled" in summary
    assert "Cause candidates" in summary
    assert "hybrid_rank_lower_than_semantic_rank" in summary
    assert "semantic_candidate_not_preserved_by_hybrid" in summary


def test_gate_records_p95_response_token_ceiling_failures() -> None:
    module = load_embedding_quality_module()
    failures: list[str] = []
    hybrid = hybrid_method(sample_methods())

    module.add_numeric_ceiling_failure(
        failures,
        hybrid,
        "p95_estimated_response_tokens",
        "P95 estimated response tokens",
        200,
    )

    assert failures == [
        "hybrid P95 estimated response tokens 250 > ceiling 200"
    ]


def test_gate_ignores_disabled_p95_response_token_ceiling() -> None:
    module = load_embedding_quality_module()
    failures: list[str] = []
    hybrid = hybrid_method(sample_methods())

    module.add_numeric_ceiling_failure(
        failures,
        hybrid,
        "p95_estimated_response_tokens",
        "P95 estimated response tokens",
        0,
    )

    assert failures == []


def run_benchmark_args(args: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(BENCHMARK_SCRIPT), *args],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=False,
    )


def test_embedding_quality_help_exposes_ranker_gate_options() -> None:
    proc = run_benchmark_args(["--help"])

    assert proc.returncode == 0
    assert "--max-hybrid-p95-response-tokens" in proc.stdout
    assert "--methods" in proc.stdout
    assert "--workers" in proc.stdout
    assert "--method-workers" in proc.stdout
    assert "--batch-size" in proc.stdout
    assert "--query-cache-probe" in proc.stdout


def test_stdout_summary_reports_disabled_query_cache_probe() -> None:
    module = load_embedding_quality_module()

    summary = module.render_stdout_summary(
        sample_result(module, query_cache_probe_enabled=False)
    )

    assert "query_cache=skipped" in summary
    assert "method_workers=3" in summary
    assert "batch_size=8" in summary
    assert "total_elapsed_ms=456.0" in summary


def test_embedding_quality_rejects_zero_workers() -> None:
    proc = run_benchmark_args(["--workers", "0"])

    assert proc.returncode != 0
    assert "positive integer" in proc.stderr


def test_parse_requested_methods_accepts_hybrid_only() -> None:
    module = load_embedding_quality_module()

    assert module.parse_requested_methods("get_ranked_context") == ["get_ranked_context"]


def test_parse_requested_methods_requires_hybrid_lane() -> None:
    module = load_embedding_quality_module()

    try:
        module.parse_requested_methods("semantic_search")
    except SystemExit as error:
        assert "get_ranked_context" in str(error)
    else:
        raise AssertionError("expected SystemExit")


def main() -> int:
    failures: list[str] = []
    for test in [
        test_ranker_diagnostics_records_cause_candidates,
        test_triage_artifact_preserves_diagnostic_cause_candidates,
        test_markdown_summary_renders_diagnostic_cause_candidates,
        test_gate_records_p95_response_token_ceiling_failures,
        test_gate_ignores_disabled_p95_response_token_ceiling,
        test_embedding_quality_help_exposes_ranker_gate_options,
        test_stdout_summary_reports_disabled_query_cache_probe,
        test_embedding_quality_rejects_zero_workers,
        test_parse_requested_methods_accepts_hybrid_only,
        test_parse_requested_methods_requires_hybrid_lane,
    ]:
        try:
            test()
            print(f"PASS  {test.__name__}")
        except AssertionError as error:
            print(f"FAIL  {test.__name__}: {error}")
            failures.append(test.__name__)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
