#!/usr/bin/env python3
"""Fail-closed promotion gate for embedding model candidates."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import tempfile
from datetime import datetime, timezone
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
ROOT = SCRIPT_DIR.parent.parent
DEFAULT_BINARY = ROOT / "target" / "release" / "codelens-mcp"
DEFAULT_OUTPUT_DIR = SCRIPT_DIR / "gate-results" / "promotion-gate"
HARNESS_REPLAY_SCRIPT = ROOT / "benchmarks" / "harness" / "replay-session-pack.py"


# ---------------------------------------------------------------------------
# Baseline identity guard
# ---------------------------------------------------------------------------


def resolve_runtime_model_dir(
    binary: str, env: dict[str, str] | None = None
) -> Path:
    """Return the model dir the binary would actually load at runtime."""
    env = env or os.environ
    exe_dir = Path(binary).resolve().parent
    candidates = [
        (
            Path(env["CODELENS_MODEL_DIR"]).expanduser().resolve() / "codesearch"
            if "CODELENS_MODEL_DIR" in env
            else None
        ),
        exe_dir / "models" / "codesearch",
        Path.home() / ".cache" / "codelens" / "models" / "codesearch",
        ROOT / "crates" / "codelens-core" / "models" / "codesearch",
    ]
    for c in candidates:
        if c is not None and (c / "model.onnx").exists():
            return c
    raise SystemExit(
        "Cannot resolve runtime model dir. Checked:\n"
        + "\n".join(f"  - {c}" for c in candidates if c)
    )


def compute_file_md5(path: Path) -> str:
    h = hashlib.md5()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def compute_file_sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def snapshot_runtime_model_identity(
    binary: str, env: dict[str, str] | None = None
) -> dict:
    """Capture runtime baseline model path, hash, and layer count."""
    model_dir = resolve_runtime_model_dir(binary, env)
    model_path = model_dir / "model.onnx"
    config_path = model_dir / "config.json"
    config = json.loads(config_path.read_text()) if config_path.exists() else {}
    return {
        "model_path": str(model_path),
        "model_dir": str(model_dir),
        "md5": compute_file_md5(model_path),
        "sha256": compute_file_sha256(model_path),
        "size_bytes": model_path.stat().st_size,
        "num_hidden_layers": config.get("num_hidden_layers"),
        "hidden_size": config.get("hidden_size"),
    }


def report_runtime_model(report: dict) -> dict | None:
    runtime_model = report.get("runtime_model")
    return runtime_model if isinstance(runtime_model, dict) else None


def assert_report_matches_identity(
    report: dict,
    expected_identity: dict,
    *,
    axis_name: str,
    report_path: Path,
) -> None:
    actual = report_runtime_model(report)
    if actual is None:
        raise SystemExit(
            f"{axis_name} report missing runtime_model fingerprint: {report_path}"
        )
    mismatches = []
    for key in ("model_path", "sha256", "num_hidden_layers"):
        if actual.get(key) != expected_identity.get(key):
            mismatches.append(
                f"{key}: report={actual.get(key)!r} expected={expected_identity.get(key)!r}"
            )
    if mismatches:
        raise SystemExit(
            f"{axis_name} report runtime_model mismatch: {report_path}\n"
            + "\n".join(f"  - {item}" for item in mismatches)
        )


def validate_candidate_not_compressed(onnx_dir: Path) -> None:
    """Block 3-layer compressed candidates; they must pass as 12-layer first."""
    config_path = onnx_dir / "config.json"
    if not config_path.exists():
        return
    config = json.loads(config_path.read_text())
    layers = config.get("num_hidden_layers")
    if layers is not None and layers < 6:
        raise SystemExit(
            f"Candidate has {layers} layers (compressed). "
            f"12-layer candidate must pass the gate before compression. "
            f"Aborting to prevent wasted compute."
        )


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--project", default=str(ROOT))
    parser.add_argument(
        "--binary", default=os.environ.get("CODELENS_BIN", str(DEFAULT_BINARY))
    )
    parser.add_argument("--candidate-onnx-dir", action="append", default=[])
    parser.add_argument("--candidate-label", action="append", default=[])
    parser.add_argument("--candidate-manifest", action="append", default=[])
    parser.add_argument("--output-dir", default=str(DEFAULT_OUTPUT_DIR))
    parser.add_argument("--baseline-only", action="store_true")
    parser.add_argument("--baseline-retrieval-report", default="")
    parser.add_argument("--baseline-harness-report", default="")
    parser.add_argument("--baseline-external-report", default="")
    parser.add_argument("--baseline-role-report", default="")
    parser.add_argument("--session-pack-json", default="")
    parser.add_argument("--replay-agent", default="codex")
    parser.add_argument("--replay-scenario-id", action="append", default=[])
    parser.add_argument("--replay-repo", action="append", default=[])
    parser.add_argument("--replay-task-kind", action="append", default=[])
    parser.add_argument("--replay-mode", action="append", default=[])
    parser.add_argument("--replay-limit", type=int, default=0)
    parser.add_argument("--allow-semantic-mrr-drop", type=float, default=0.0)
    parser.add_argument("--allow-hybrid-mrr-drop", type=float, default=0.0)
    parser.add_argument("--allow-hybrid-acc1-drop", type=float, default=0.0)
    parser.add_argument("--allow-external-semantic-mrr-drop", type=float, default=0.0)
    parser.add_argument("--allow-external-hybrid-mrr-drop", type=float, default=0.0)
    parser.add_argument("--allow-external-hybrid-acc1-drop", type=float, default=0.0)
    parser.add_argument("--allow-role-semantic-mrr-drop", type=float, default=0.0)
    parser.add_argument("--allow-role-hybrid-mrr-drop", type=float, default=0.0)
    parser.add_argument("--allow-role-hybrid-acc1-drop", type=float, default=0.0)
    parser.add_argument("--allow-task-success-drop", type=float, default=0.0)
    parser.add_argument("--max-token-per-success-increase", type=float, default=-1.0)
    parser.add_argument(
        "--max-latency-per-success-increase-ms", type=float, default=-1.0
    )
    parser.add_argument("--min-real-session-tasks", type=int, default=20)
    parser.add_argument("--min-real-session-scopes", type=int, default=3)
    return parser.parse_args()


def load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def run(
    cmd: list[str], *, env: dict[str, str] | None = None
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=ROOT,
        env=env or os.environ.copy(),
        text=True,
        capture_output=True,
        check=False,
    )


def require_success(result: subprocess.CompletedProcess[str], name: str) -> None:
    if result.returncode == 0:
        return
    raise SystemExit(
        f"{name} failed\nstdout:\n{result.stdout}\n\nstderr:\n{result.stderr}"
    )


def stage_candidate_model(
    onnx_dir: Path, candidate_label: str
) -> tuple[Path, dict[str, str]]:
    if not onnx_dir.exists():
        raise SystemExit(f"Candidate ONNX dir not found: {onnx_dir}")
    temp_root = Path(tempfile.mkdtemp(prefix="codelens-promotion-gate-"))
    model_root = temp_root / "codesearch"
    shutil.copytree(onnx_dir, model_root)
    env = os.environ.copy()
    env["CODELENS_MODEL_DIR"] = str(temp_root)
    env["CODELENS_EMBED_MODEL"] = candidate_label
    return temp_root, env


def run_retrieval_benchmark(
    project: str,
    binary: str,
    output_json: Path,
    output_md: Path,
    *,
    env: dict[str, str] | None,
) -> None:
    cmd = [
        "python3",
        "benchmarks/embedding-quality.py",
        project,
        "--binary",
        binary,
        "--isolated-copy",
        "--output",
        str(output_json),
        "--markdown-output",
        str(output_md),
    ]
    require_success(run(cmd, env=env), "embedding-quality.py")


def run_harness_benchmark(
    project: str,
    binary: str,
    output_json: Path,
    output_md: Path,
    *,
    env: dict[str, str] | None,
) -> None:
    cmd = [
        "python3",
        "benchmarks/harness/harness-eval.py",
        "--repo",
        project,
        "--binary",
        binary,
        "--output-json",
        str(output_json),
        "--output-md",
        str(output_md),
        "--label",
        output_json.stem,
    ]
    require_success(run(cmd, env=env), "harness-eval.py")


def run_harness_replay(
    session_pack_json: Path,
    binary: str,
    output_dir: Path,
    *,
    agent: str,
    scenario_ids: list[str],
    repos: list[str],
    task_kinds: list[str],
    modes: list[str],
    limit: int,
    env: dict[str, str] | None,
) -> dict:
    output_dir.mkdir(parents=True, exist_ok=True)
    cmd = [
        "python3",
        str(HARNESS_REPLAY_SCRIPT),
        "--session-pack-json",
        str(session_pack_json),
        "--agent",
        agent,
        "--binary",
        binary,
        "--output-dir",
        str(output_dir),
        "--label",
        output_dir.name,
    ]
    for scenario_id in scenario_ids:
        cmd.extend(["--scenario-id", scenario_id])
    for repo in repos:
        cmd.extend(["--repo", repo])
    for task_kind in task_kinds:
        cmd.extend(["--task-kind", task_kind])
    for mode in modes:
        cmd.extend(["--mode", mode])
    if limit > 0:
        cmd.extend(["--limit", str(limit)])
    result = run(cmd, env=env)
    require_success(result, "replay-session-pack.py")
    summary_path = output_dir / "replay-summary.json"
    if not summary_path.exists():
        raise SystemExit(f"Missing replay summary: {summary_path}")
    return load_json(summary_path)


def run_real_session_evidence(
    binary: str,
    retrieval_report: Path,
    output_json: Path,
    output_md: Path,
    *,
    harness_report: Path | None,
    min_real_session_tasks: int,
    min_real_session_scopes: int,
    env: dict[str, str] | None,
    session_entry_globs: list[str] | None = None,
    no_default_session_glob: bool = False,
    no_refresh_existing_report: bool = False,
) -> None:
    cmd = [
        "python3",
        "benchmarks/harness/real-session-evidence.py",
        "--binary",
        binary,
        "--retrieval-report",
        str(retrieval_report),
        "--min-real-session-tasks",
        str(min_real_session_tasks),
        "--min-real-session-scopes",
        str(min_real_session_scopes),
        "--output-json",
        str(output_json),
        "--output-md",
        str(output_md),
    ]
    if harness_report is not None:
        cmd.extend(["--harness-report", str(harness_report)])
    if no_default_session_glob:
        cmd.append("--no-default-session-glob")
    if no_refresh_existing_report:
        cmd.append("--no-refresh-existing-report")
    for pattern in session_entry_globs or []:
        cmd.extend(["--session-entry-glob", pattern])
    require_success(run(cmd, env=env), "real-session-evidence.py")


def run_paper_benchmark(
    harness_report: Path,
    retrieval_report: Path,
    output_json: Path,
    output_md: Path,
    *,
    min_real_session_tasks: int,
    min_real_session_scopes: int,
) -> None:
    cmd = [
        "python3",
        "benchmarks/paper-benchmark.py",
        "--harness-report",
        str(harness_report),
        "--retrieval-report",
        str(retrieval_report),
        "--mode",
        "routed-on",
        "--output-json",
        str(output_json),
        "--output-md",
        str(output_md),
        "--min-real-session-tasks",
        str(min_real_session_tasks),
        "--min-real-session-scopes",
        str(min_real_session_scopes),
    ]
    require_success(run(cmd), "paper-benchmark.py")


def run_external_benchmark(
    binary: str, output_json: Path, output_md: Path, *, env: dict[str, str] | None
) -> None:
    cmd = [
        "python3",
        "benchmarks/external-retrieval.py",
        "--binary",
        binary,
        "--isolated-copy",
        "--output",
        str(output_json),
        "--markdown-output",
        str(output_md),
    ]
    require_success(run(cmd, env=env), "external-retrieval.py")


def run_role_benchmark(
    project: str,
    binary: str,
    output_json: Path,
    output_md: Path,
    *,
    env: dict[str, str] | None,
) -> None:
    cmd = [
        "python3",
        "benchmarks/role-retrieval.py",
        project,
        "--binary",
        binary,
        "--isolated-copy",
        "--output",
        str(output_json),
        "--markdown-output",
        str(output_md),
    ]
    require_success(run(cmd, env=env), "role-retrieval.py")


def run_contamination_audit(manifest: Path, output_json: Path) -> None:
    cmd = [
        "python3",
        "scripts/finetune/contamination_audit.py",
        "--manifest",
        str(manifest),
        "--output",
        str(output_json),
    ]
    result = run(cmd)
    if output_json.exists():
        return
    require_success(result, "contamination_audit.py")


def method(report: dict, name: str) -> dict:
    for item in report.get("methods", []):
        if item.get("method") == name:
            return item
    raise SystemExit(f"Report missing method={name}")


def row_key(row: dict) -> tuple:
    if "repo_id" in row:
        return (row.get("repo_id"), row.get("query"))
    return (row.get("query"),)


def worst_rank_regressions(
    baseline_report: dict, candidate_report: dict, method_name: str, limit: int = 8
) -> list[dict]:
    baseline_rows = {
        row_key(row): row
        for row in method(baseline_report, method_name).get("rows", [])
    }
    candidate_rows = {
        row_key(row): row
        for row in method(candidate_report, method_name).get("rows", [])
    }
    worst = []
    for key, base_row in baseline_rows.items():
        cand_row = candidate_rows.get(key)
        if not cand_row:
            continue
        base_rank = base_row.get("rank")
        cand_rank = cand_row.get("rank")
        base_score = 999 if base_rank is None else base_rank
        cand_score = 999 if cand_rank is None else cand_rank
        delta = cand_score - base_score
        if delta <= 0:
            continue
        worst.append(
            {
                "query": base_row.get("query"),
                "repo_id": base_row.get("repo_id"),
                "expected_symbol": base_row.get("expected_symbol"),
                "baseline_rank": base_rank,
                "candidate_rank": cand_rank,
                "baseline_top_candidate": base_row.get("top_candidate"),
                "candidate_top_candidate": cand_row.get("top_candidate"),
                "rank_delta": delta,
            }
        )
    worst.sort(
        key=lambda item: (-item["rank_delta"], item.get("repo_id") or "", item["query"])
    )
    return worst[:limit]


def compare_retrieval_axis(
    axis_name: str,
    baseline_report: dict,
    candidate_report: dict,
    *,
    allow_semantic_mrr_drop: float,
    allow_hybrid_mrr_drop: float,
    allow_hybrid_acc1_drop: float,
) -> dict:
    failures = []
    warnings = []
    base_semantic = method(baseline_report, "semantic_search")
    cand_semantic = method(candidate_report, "semantic_search")
    base_hybrid = method(baseline_report, "get_ranked_context")
    cand_hybrid = method(candidate_report, "get_ranked_context")

    if (
        candidate_report.get("sufficient_evidence") is False
        or baseline_report.get("sufficient_evidence") is False
    ):
        failures.append(
            "insufficient benchmark evidence: "
            f"baseline={baseline_report.get('sufficient_evidence')}, "
            f"candidate={candidate_report.get('sufficient_evidence')}"
        )
    if (cand_semantic.get("mrr") or 0.0) + allow_semantic_mrr_drop < (
        base_semantic.get("mrr") or 0.0
    ):
        failures.append(
            "semantic_search MRR regressed: "
            f"{cand_semantic.get('mrr', 0.0):.3f} < {base_semantic.get('mrr', 0.0):.3f}"
        )
    if (cand_hybrid.get("mrr") or 0.0) + allow_hybrid_mrr_drop < (
        base_hybrid.get("mrr") or 0.0
    ):
        failures.append(
            "get_ranked_context MRR regressed: "
            f"{cand_hybrid.get('mrr', 0.0):.3f} < {base_hybrid.get('mrr', 0.0):.3f}"
        )
    if (cand_hybrid.get("acc1") or 0.0) + allow_hybrid_acc1_drop < (
        base_hybrid.get("acc1") or 0.0
    ):
        failures.append(
            "get_ranked_context Acc@1 regressed: "
            f"{cand_hybrid.get('acc1', 0.0):.3f} < {base_hybrid.get('acc1', 0.0):.3f}"
        )

    return {
        "axis": axis_name,
        "passed": not failures,
        "failures": failures,
        "warnings": warnings,
        "deltas": {
            "semantic_search_mrr": (cand_semantic.get("mrr") or 0.0)
            - (base_semantic.get("mrr") or 0.0),
            "get_ranked_context_mrr": (cand_hybrid.get("mrr") or 0.0)
            - (base_hybrid.get("mrr") or 0.0),
            "get_ranked_context_acc1": (cand_hybrid.get("acc1") or 0.0)
            - (base_hybrid.get("acc1") or 0.0),
        },
        "worst_regressions": {
            "semantic_search": worst_rank_regressions(
                baseline_report, candidate_report, "semantic_search"
            ),
            "get_ranked_context": worst_rank_regressions(
                baseline_report, candidate_report, "get_ranked_context"
            ),
        },
    }


def compare_harness_axis(args, baseline_paper: dict, candidate_paper: dict) -> dict:
    failures = []
    warnings = []
    base_eligibility = baseline_paper.get("promotion_eligibility", {})
    cand_eligibility = candidate_paper.get("promotion_eligibility", {})
    if not base_eligibility.get("promotion_eligible"):
        failures.append(
            "baseline harness evidence insufficient: "
            + "; ".join(base_eligibility.get("failures", []) or ["unknown"])
        )
    if not cand_eligibility.get("promotion_eligible"):
        failures.append(
            "candidate harness evidence insufficient: "
            + "; ".join(cand_eligibility.get("failures", []) or ["unknown"])
        )

    base_success = baseline_paper["harness_metrics"].get("task_success_rate")
    cand_success = candidate_paper["harness_metrics"].get("task_success_rate")
    if base_success is not None and cand_success is not None:
        if cand_success + args.allow_task_success_drop < base_success:
            failures.append(
                "task success rate regressed: "
                f"{cand_success:.3f} < {base_success:.3f}"
            )

    base_tokens = baseline_paper["harness_metrics"].get("tokens_per_successful_task")
    cand_tokens = candidate_paper["harness_metrics"].get("tokens_per_successful_task")
    if base_tokens is not None and cand_tokens is not None:
        if (
            args.max_token_per_success_increase >= 0
            and cand_tokens > base_tokens + args.max_token_per_success_increase
        ):
            failures.append(
                "tokens per successful task increased beyond threshold: "
                f"{cand_tokens:.1f} > {base_tokens:.1f} + {args.max_token_per_success_increase:.1f}"
            )
        elif cand_tokens > base_tokens:
            warnings.append(
                "tokens per successful task increased: "
                f"{cand_tokens:.1f} > {base_tokens:.1f}"
            )

    base_latency = baseline_paper["harness_metrics"].get(
        "latency_per_successful_task_ms"
    )
    cand_latency = candidate_paper["harness_metrics"].get(
        "latency_per_successful_task_ms"
    )
    if base_latency is not None and cand_latency is not None:
        if (
            args.max_latency_per_success_increase_ms >= 0
            and cand_latency > base_latency + args.max_latency_per_success_increase_ms
        ):
            failures.append(
                "latency per successful task increased beyond threshold: "
                f"{cand_latency:.1f} > {base_latency:.1f} + {args.max_latency_per_success_increase_ms:.1f}"
            )
        elif cand_latency > base_latency:
            warnings.append(
                "latency per successful task increased: "
                f"{cand_latency:.1f} > {base_latency:.1f}"
            )

    return {
        "axis": "harness",
        "passed": not failures,
        "failures": failures,
        "warnings": warnings,
        "deltas": {
            "task_success_rate": (
                None
                if base_success is None or cand_success is None
                else cand_success - base_success
            ),
            "tokens_per_successful_task": (
                None
                if base_tokens is None or cand_tokens is None
                else cand_tokens - base_tokens
            ),
            "latency_per_successful_task_ms": (
                None
                if base_latency is None or cand_latency is None
                else cand_latency - base_latency
            ),
        },
        "baseline_selected_cohort": baseline_paper.get("selected_cohort"),
        "candidate_selected_cohort": candidate_paper.get("selected_cohort"),
        "baseline_coverage_gap_queue": baseline_paper.get("coverage_gap_queue"),
        "candidate_coverage_gap_queue": candidate_paper.get("coverage_gap_queue"),
    }


def compare_contamination_axis(report: dict | None) -> dict:
    failures = []
    if report is None:
        failures.append("candidate contamination audit missing")
    elif not report.get("passed"):
        failures.extend(report.get("failures", []) or ["contamination audit failed"])
    return {
        "axis": "contamination",
        "passed": not failures,
        "failures": failures,
        "warnings": [],
        "finding_count": None if report is None else report.get("finding_count"),
        "overlap_counts": None if report is None else report.get("overlap_counts"),
    }


def ensure_report(path_arg: str, default_path: Path, runner) -> Path:
    if path_arg:
        return Path(path_arg).expanduser().resolve()
    runner(default_path)
    return default_path


def collect_baseline_reports(
    args, project: str, binary: str, baseline_dir: Path
) -> dict[str, Path]:
    baseline_dir.mkdir(parents=True, exist_ok=True)
    reports = {}
    replay_summary = None

    reports["retrieval"] = ensure_report(
        args.baseline_retrieval_report,
        baseline_dir / "embedding-quality.json",
        lambda path: run_retrieval_benchmark(
            project, binary, path, baseline_dir / "embedding-quality.md", env=None
        ),
    )
    if args.session_pack_json:
        replay_summary = run_harness_replay(
            Path(args.session_pack_json).expanduser().resolve(),
            binary,
            baseline_dir / "harness-replay",
            agent=args.replay_agent,
            scenario_ids=args.replay_scenario_id,
            repos=args.replay_repo,
            task_kinds=args.replay_task_kind,
            modes=args.replay_mode,
            limit=args.replay_limit,
            env=None,
        )
        raw_harness_report = Path(replay_summary["harness_report_json"]).resolve()
        session_entry_globs = [replay_summary["session_entry_glob"]]
        no_default_session_glob = True
        no_refresh_existing_report = True
    else:
        raw_harness_report = (
            Path(args.baseline_harness_report).expanduser().resolve()
            if args.baseline_harness_report
            else None
        )
        session_entry_globs = None
        no_default_session_glob = False
        no_refresh_existing_report = False
    reports["harness"] = baseline_dir / "real-session-evidence.json"
    run_real_session_evidence(
        binary,
        reports["retrieval"],
        reports["harness"],
        baseline_dir / "real-session-evidence.md",
        harness_report=raw_harness_report,
        min_real_session_tasks=args.min_real_session_tasks,
        min_real_session_scopes=args.min_real_session_scopes,
        env=None,
        session_entry_globs=session_entry_globs,
        no_default_session_glob=no_default_session_glob,
        no_refresh_existing_report=no_refresh_existing_report,
    )
    reports["paper"] = reports["harness"]
    reports["external"] = ensure_report(
        args.baseline_external_report,
        baseline_dir / "external-retrieval.json",
        lambda path: run_external_benchmark(
            binary, path, baseline_dir / "external-retrieval.md", env=None
        ),
    )
    reports["role"] = ensure_report(
        args.baseline_role_report,
        baseline_dir / "role-retrieval.json",
        lambda path: run_role_benchmark(
            project, binary, path, baseline_dir / "role-retrieval.md", env=None
        ),
    )
    if replay_summary is not None:
        reports["harness_replay"] = Path(
            replay_summary["harness_report_json"]
        ).resolve()
        reports["harness_replay_summary"] = (
            baseline_dir / "harness-replay" / "replay-summary.json"
        )
    return reports


def resolve_candidates(args) -> list[dict]:
    onnx_dirs = [Path(path).expanduser().resolve() for path in args.candidate_onnx_dir]
    labels = list(args.candidate_label)
    manifests = [Path(path).expanduser().resolve() for path in args.candidate_manifest]
    if args.baseline_only:
        return []
    if not onnx_dirs:
        raise SystemExit(
            "Need at least one --candidate-onnx-dir or use --baseline-only"
        )
    if labels and len(labels) not in {0, len(onnx_dirs)}:
        raise SystemExit(
            "Pass either zero labels or one --candidate-label per --candidate-onnx-dir"
        )
    if manifests and len(manifests) not in {0, len(onnx_dirs)}:
        raise SystemExit(
            "Pass either zero manifests or one --candidate-manifest per --candidate-onnx-dir"
        )
    candidates = []
    for index, onnx_dir in enumerate(onnx_dirs):
        label = labels[index] if index < len(labels) else onnx_dir.parent.name
        manifest = manifests[index] if index < len(manifests) else None
        candidates.append(
            {
                "label": label,
                "onnx_dir": onnx_dir,
                "manifest": manifest,
            }
        )
    return candidates


def evaluate_candidate(
    args, spec: dict, project: str, binary: str, output_dir: Path
) -> tuple[dict, Path]:
    cleanup_root, env = stage_candidate_model(spec["onnx_dir"], spec["label"])
    candidate_identity = snapshot_runtime_model_identity(binary, env)
    output_dir.mkdir(parents=True, exist_ok=True)
    try:
        retrieval_path = output_dir / "embedding-quality.json"
        harness_path = output_dir / "real-session-evidence.json"
        external_path = output_dir / "external-retrieval.json"
        role_path = output_dir / "role-retrieval.json"
        contamination_path = output_dir / "contamination-audit.json"
        replay_summary = None

        run_retrieval_benchmark(
            project,
            binary,
            retrieval_path,
            output_dir / "embedding-quality.md",
            env=env,
        )
        if args.session_pack_json:
            replay_summary = run_harness_replay(
                Path(args.session_pack_json).expanduser().resolve(),
                binary,
                output_dir / "harness-replay",
                agent=args.replay_agent,
                scenario_ids=args.replay_scenario_id,
                repos=args.replay_repo,
                task_kinds=args.replay_task_kind,
                modes=args.replay_mode,
                limit=args.replay_limit,
                env=env,
            )
            raw_harness_report = Path(replay_summary["harness_report_json"]).resolve()
            session_entry_globs = [replay_summary["session_entry_glob"]]
            no_default_session_glob = True
            no_refresh_existing_report = True
        else:
            raw_harness_report = None
            session_entry_globs = None
            no_default_session_glob = False
            no_refresh_existing_report = False
        run_real_session_evidence(
            binary,
            retrieval_path,
            harness_path,
            output_dir / "real-session-evidence.md",
            harness_report=raw_harness_report,
            min_real_session_tasks=args.min_real_session_tasks,
            min_real_session_scopes=args.min_real_session_scopes,
            env=env,
            session_entry_globs=session_entry_globs,
            no_default_session_glob=no_default_session_glob,
            no_refresh_existing_report=no_refresh_existing_report,
        )
        run_external_benchmark(
            binary, external_path, output_dir / "external-retrieval.md", env=env
        )
        run_role_benchmark(
            project, binary, role_path, output_dir / "role-retrieval.md", env=env
        )
        contamination_report = None
        if spec.get("manifest"):
            run_contamination_audit(spec["manifest"], contamination_path)
            contamination_report = load_json(contamination_path)
        retrieval_artifact = load_json(retrieval_path)
        external_artifact = load_json(external_path)
        role_artifact = load_json(role_path)
        assert_report_matches_identity(
            retrieval_artifact,
            candidate_identity,
            axis_name=f"{spec['label']} product retrieval",
            report_path=retrieval_path,
        )
        assert_report_matches_identity(
            external_artifact,
            candidate_identity,
            axis_name=f"{spec['label']} external retrieval",
            report_path=external_path,
        )
        assert_report_matches_identity(
            role_artifact,
            candidate_identity,
            axis_name=f"{spec['label']} role retrieval",
            report_path=role_path,
        )
        artifacts = {
            "retrieval": retrieval_artifact,
            "harness": load_json(harness_path),
            "paper": load_json(harness_path),
            "external": external_artifact,
            "role": role_artifact,
            "contamination": contamination_report,
        }
        artifact_paths = {
            "retrieval": str(retrieval_path),
            "harness": str(harness_path),
            "paper": str(harness_path),
            "external": str(external_path),
            "role": str(role_path),
            "contamination": str(contamination_path) if spec.get("manifest") else None,
            "harness_replay": (
                replay_summary.get("harness_report_json") if replay_summary else None
            ),
            "harness_replay_summary": (
                str(output_dir / "harness-replay" / "replay-summary.json")
                if replay_summary
                else None
            ),
        }
        return {
            "artifacts": artifacts,
            "paths": artifact_paths,
            "candidate_identity": candidate_identity,
        }, cleanup_root
    except Exception:
        shutil.rmtree(cleanup_root, ignore_errors=True)
        raise


def candidate_selection_key(candidate_report: dict) -> tuple[float, float, float]:
    product = candidate_report["candidate"]["product_retrieval"]["artifacts"]
    role = candidate_report["candidate"]["role_retrieval"]["artifacts"]
    return (
        method(product, "get_ranked_context").get("mrr") or 0.0,
        method(product, "semantic_search").get("mrr") or 0.0,
        method(role, "get_ranked_context").get("mrr") or 0.0,
    )


def main():
    args = parse_args()
    output_dir = Path(args.output_dir).expanduser().resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    project = str(Path(args.project).expanduser().resolve())
    binary = str(Path(args.binary).expanduser().resolve())

    # --- Baseline identity guard ---
    baseline_id = snapshot_runtime_model_identity(binary)
    print(
        f"Baseline identity: {baseline_id['num_hidden_layers']}L "
        f"hidden={baseline_id['hidden_size']} "
        f"size={baseline_id['size_bytes'] // (1024 * 1024)}MB "
        f"sha256={baseline_id['sha256'][:12]}… "
        f"path={baseline_id['model_path']}"
    )

    # --- Candidate compression guard ---
    candidates = resolve_candidates(args)
    for spec in candidates:
        validate_candidate_not_compressed(spec["onnx_dir"])

    baseline_paths = collect_baseline_reports(
        args, project, binary, output_dir / "baseline"
    )
    baseline_artifacts = {
        name: load_json(path) for name, path in baseline_paths.items()
    }
    assert_report_matches_identity(
        baseline_artifacts["retrieval"],
        baseline_id,
        axis_name="baseline product retrieval",
        report_path=baseline_paths["retrieval"],
    )
    assert_report_matches_identity(
        baseline_artifacts["external"],
        baseline_id,
        axis_name="baseline external retrieval",
        report_path=baseline_paths["external"],
    )
    assert_report_matches_identity(
        baseline_artifacts["role"],
        baseline_id,
        axis_name="baseline role retrieval",
        report_path=baseline_paths["role"],
    )
    candidate_reports = []
    cleanup_dirs: list[Path] = []
    try:
        for spec in candidates:
            candidate_output_dir = output_dir / spec["label"]
            evaluated, cleanup_root = evaluate_candidate(
                args, spec, project, binary, candidate_output_dir
            )
            cleanup_dirs.append(cleanup_root)

            product_axis = compare_retrieval_axis(
                "product_retrieval",
                baseline_artifacts["retrieval"],
                evaluated["artifacts"]["retrieval"],
                allow_semantic_mrr_drop=args.allow_semantic_mrr_drop,
                allow_hybrid_mrr_drop=args.allow_hybrid_mrr_drop,
                allow_hybrid_acc1_drop=args.allow_hybrid_acc1_drop,
            )
            external_axis = compare_retrieval_axis(
                "external_retrieval",
                baseline_artifacts["external"],
                evaluated["artifacts"]["external"],
                allow_semantic_mrr_drop=args.allow_external_semantic_mrr_drop,
                allow_hybrid_mrr_drop=args.allow_external_hybrid_mrr_drop,
                allow_hybrid_acc1_drop=args.allow_external_hybrid_acc1_drop,
            )
            role_axis = compare_retrieval_axis(
                "role_retrieval",
                baseline_artifacts["role"],
                evaluated["artifacts"]["role"],
                allow_semantic_mrr_drop=args.allow_role_semantic_mrr_drop,
                allow_hybrid_mrr_drop=args.allow_role_hybrid_mrr_drop,
                allow_hybrid_acc1_drop=args.allow_role_hybrid_acc1_drop,
            )
            harness_axis = compare_harness_axis(
                args, baseline_artifacts["paper"], evaluated["artifacts"]["paper"]
            )
            contamination_axis = compare_contamination_axis(
                evaluated["artifacts"]["contamination"]
            )
            axes = {
                "product_retrieval": product_axis,
                "external_retrieval": external_axis,
                "role_retrieval": role_axis,
                "harness": harness_axis,
                "contamination": contamination_axis,
            }
            candidate_reports.append(
                {
                    "candidate_label": spec["label"],
                    "candidate_onnx_dir": str(spec["onnx_dir"]),
                    "candidate_manifest": (
                        str(spec["manifest"]) if spec.get("manifest") else None
                    ),
                    "candidate_identity": evaluated["candidate_identity"],
                    "passed": all(axis["passed"] for axis in axes.values()),
                    "axes": axes,
                    "candidate": {
                        "product_retrieval": {
                            "artifacts": evaluated["artifacts"]["retrieval"],
                            "path": evaluated["paths"]["retrieval"],
                        },
                        "external_retrieval": {
                            "artifacts": evaluated["artifacts"]["external"],
                            "path": evaluated["paths"]["external"],
                        },
                        "role_retrieval": {
                            "artifacts": evaluated["artifacts"]["role"],
                            "path": evaluated["paths"]["role"],
                        },
                        "paper_benchmark": {
                            "artifacts": evaluated["artifacts"]["paper"],
                            "path": evaluated["paths"]["paper"],
                        },
                        "harness_replay": {
                            "path": evaluated["paths"]["harness_replay"],
                            "summary_path": evaluated["paths"][
                                "harness_replay_summary"
                            ],
                        },
                        "contamination": {
                            "artifacts": evaluated["artifacts"]["contamination"],
                            "path": evaluated["paths"]["contamination"],
                        },
                    },
                }
            )

        selected = None
        passing_candidates = [
            candidate for candidate in candidate_reports if candidate["passed"]
        ]
        if passing_candidates:
            selected = max(passing_candidates, key=candidate_selection_key)

        report = {
            "schema_version": "codelens-promotion-gate-v5",
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "baseline_identity": baseline_id,
            "inputs": {
                "project": project,
                "binary": binary,
                "baseline_only": args.baseline_only,
                "candidate_count": len(candidates),
                "min_real_session_tasks": args.min_real_session_tasks,
                "min_real_session_scopes": args.min_real_session_scopes,
                "session_pack_json": (
                    str(Path(args.session_pack_json).expanduser().resolve())
                    if args.session_pack_json
                    else None
                ),
                "replay_agent": args.replay_agent if args.session_pack_json else None,
                "replay_limit": args.replay_limit if args.session_pack_json else None,
            },
            "baseline": {
                "retrieval_report": str(baseline_paths["retrieval"]),
                "harness_report": str(baseline_paths["harness"]),
                "paper_report": str(baseline_paths["paper"]),
                "external_report": str(baseline_paths["external"]),
                "role_report": str(baseline_paths["role"]),
                "harness_replay_report": (
                    str(baseline_paths["harness_replay"])
                    if baseline_paths.get("harness_replay")
                    else None
                ),
                "harness_replay_summary": (
                    str(baseline_paths["harness_replay_summary"])
                    if baseline_paths.get("harness_replay_summary")
                    else None
                ),
                "product_retrieval": baseline_artifacts["retrieval"],
                "paper_benchmark": baseline_artifacts["paper"],
                "external_retrieval": baseline_artifacts["external"],
                "role_retrieval": baseline_artifacts["role"],
            },
            "candidates": candidate_reports,
            "selected_candidate_label": (
                selected["candidate_label"] if selected else None
            ),
            "selected_candidate_selection_key": (
                candidate_selection_key(selected) if selected else None
            ),
            "passed": bool(selected),
        }
        report_path = output_dir / "promotion-gate-report.json"
        report_path.write_text(
            json.dumps(report, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
        print(json.dumps(report, ensure_ascii=False, indent=2))
        if args.baseline_only:
            return
        if not selected:
            raise SystemExit(1)
    finally:
        for path in cleanup_dirs:
            shutil.rmtree(path, ignore_errors=True)


if __name__ == "__main__":
    main()
