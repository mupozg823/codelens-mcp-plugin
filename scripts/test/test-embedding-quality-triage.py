#!/usr/bin/env -S uv run --script
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
        "mrr": 0.7,
        "recall_at_cutoff": 0.8,
        "acc1": 0.6,
        "acc3": 0.7,
        "acc5": 0.8,
        "avg_elapsed_ms": 10.0,
        "p95_elapsed_ms": 20.0,
        "avg_estimated_response_tokens": 100,
        "p95_estimated_response_tokens": 250,
        "avg_response_bytes": 400,
        "p95_response_bytes": 800,
    }


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


def sample_result(module: ModuleType) -> dict[str, object]:
    methods = sample_methods()
    return {
        "project": "/repo",
        "benchmark_project": "/repo",
        "binary": "/repo/target/debug/codelens-mcp",
        "dataset_path": "/repo/benchmarks/embedding-quality-dataset-self.json",
        "dataset_size": 3,
        "ranking_cutoff": 10,
        "embedding_model": "MiniLM-L12-CodeSearchNet-INT8",
        "requested_embed_model": None,
        "methods": methods,
        "hybrid_uplift": {
            "mrr_delta": 0.1,
            "acc1_delta": 0.1,
            "acc3_delta": 0.1,
            "acc5_delta": 0.1,
        },
        "hybrid_uplift_by_query_type": {},
        "ranker_diagnostics": module.ranker_diagnostics(methods),
        "query_cache_probe": {
            "query": "demoted",
            "first_elapsed_ms": 20.0,
            "second_elapsed_ms": 10.0,
            "first_cache_hit_tier": None,
            "cache_hit_signal_available": True,
            "cache_hit_observed": True,
            "second_cache_hit_tier": "warm",
        },
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
    rows = diagnostics["rows"]

    assert_cause_candidates(
        [
            diagnostic_row
            for diagnostic_row in rows
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

    assert artifact["token_budget"]["p95_response_tokens"] == 250
    assert artifact["query_cache_probe"]["cache_hit_observed"] is True
    assert_cause_candidates(artifact["candidate_missing"]["rows"])
    assert_cause_candidates(artifact["semantic_hit_dropped_by_hybrid"]["rows"])
    assert_cause_candidates(artifact["hybrid_demoted_semantic_hit"]["rows"])


def test_markdown_summary_renders_diagnostic_cause_candidates() -> None:
    module = load_embedding_quality_module()

    summary = module.render_markdown(sample_result(module))

    assert "Cause candidates" in summary
    assert "hybrid_rank_lower_than_semantic_rank" in summary
    assert "semantic_candidate_not_preserved_by_hybrid" in summary


def main() -> int:
    failures: list[str] = []
    for test in [
        test_ranker_diagnostics_records_cause_candidates,
        test_triage_artifact_preserves_diagnostic_cause_candidates,
        test_markdown_summary_renders_diagnostic_cause_candidates,
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
