#!/usr/bin/env python3
"""Promotion gate for candidate embedding models.

This gate compares a candidate model against the currently deployed runtime model
using fresh or pre-generated reports across:
  1. Product retrieval benchmark (`benchmarks/embedding-quality.py`)
  2. Harness/paper benchmark (`benchmarks/harness/harness-eval.py` + `benchmarks/paper-benchmark.py`)

The goal is to stop models with strong internal validation but weaker product
retrieval from being promoted.
"""

from __future__ import annotations

import argparse
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


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--project", default=str(ROOT))
    parser.add_argument("--binary", default=os.environ.get("CODELENS_BIN", str(DEFAULT_BINARY)))
    parser.add_argument("--candidate-onnx-dir", default="")
    parser.add_argument("--candidate-label", default="")
    parser.add_argument("--output-dir", default=str(DEFAULT_OUTPUT_DIR))
    parser.add_argument("--baseline-retrieval-report", default="")
    parser.add_argument("--candidate-retrieval-report", default="")
    parser.add_argument("--baseline-harness-report", default="")
    parser.add_argument("--candidate-harness-report", default="")
    parser.add_argument(
        "--allow-semantic-mrr-drop",
        type=float,
        default=0.0,
        help="Allowed semantic_search MRR regression before failing.",
    )
    parser.add_argument(
        "--allow-hybrid-mrr-drop",
        type=float,
        default=0.0,
        help="Allowed get_ranked_context MRR regression before failing.",
    )
    parser.add_argument(
        "--allow-hybrid-acc1-drop",
        type=float,
        default=0.0,
        help="Allowed get_ranked_context Acc@1 regression before failing.",
    )
    parser.add_argument(
        "--allow-task-success-drop",
        type=float,
        default=0.0,
        help="Allowed task success rate regression before failing.",
    )
    parser.add_argument(
        "--max-token-per-success-increase",
        type=float,
        default=-1.0,
        help="Fail if tokens per successful task exceed baseline by more than this amount. Negative disables.",
    )
    parser.add_argument(
        "--max-latency-per-success-increase-ms",
        type=float,
        default=-1.0,
        help="Fail if latency per successful task exceeds baseline by more than this amount. Negative disables.",
    )
    parser.add_argument(
        "--skip-harness",
        action="store_true",
        help="Skip harness + paper benchmark comparison.",
    )
    parser.add_argument(
        "--include-real-sessions",
        action="store_true",
        help="Include archived real-session entries in the harness benchmark instead of synthetic-only.",
    )
    parser.add_argument(
        "--require-real-session-harness",
        action="store_true",
        help="Fail if the selected paper benchmark cohort is not real-session.",
    )
    return parser.parse_args()


def load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def run(cmd: list[str], *, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
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


def stage_candidate_model(onnx_dir: Path, candidate_label: str) -> tuple[Path, dict[str, str]]:
    if not onnx_dir.exists():
        raise SystemExit(f"Candidate ONNX dir not found: {onnx_dir}")
    temp_root = Path(tempfile.mkdtemp(prefix="codelens-promotion-gate-"))
    model_root = temp_root / "codesearch"
    shutil.copytree(onnx_dir, model_root)
    env = os.environ.copy()
    env["CODELENS_MODEL_DIR"] = str(temp_root)
    env["CODELENS_EMBED_MODEL"] = candidate_label
    return temp_root, env


def run_retrieval_benchmark(project: str, binary: str, output_json: Path, output_md: Path, *, env: dict[str, str] | None) -> None:
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


def run_harness_benchmark(project: str, binary: str, output_json: Path, output_md: Path, *, env: dict[str, str] | None, include_real_sessions: bool) -> None:
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
    if not include_real_sessions:
        cmd.append("--skip-real-sessions")
    require_success(run(cmd, env=env), "harness-eval.py")


def run_paper_benchmark(harness_report: Path, retrieval_report: Path, output_json: Path, output_md: Path) -> None:
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
    ]
    require_success(run(cmd), "paper-benchmark.py")


def method(report: dict, name: str) -> dict:
    for item in report.get("methods", []):
        if item.get("method") == name:
            return item
    raise SystemExit(f"Retrieval report missing method={name}")


def compare_metrics(
    args,
    baseline_retrieval: dict,
    candidate_retrieval: dict,
    baseline_paper: dict | None,
    candidate_paper: dict | None,
) -> tuple[list[str], list[str], dict]:
    failures: list[str] = []
    warnings: list[str] = []

    base_semantic = method(baseline_retrieval, "semantic_search")
    cand_semantic = method(candidate_retrieval, "semantic_search")
    base_hybrid = method(baseline_retrieval, "get_ranked_context")
    cand_hybrid = method(candidate_retrieval, "get_ranked_context")

    if (cand_semantic.get("mrr") or 0.0) + args.allow_semantic_mrr_drop < (base_semantic.get("mrr") or 0.0):
        failures.append(
            "semantic_search MRR regressed: "
            f"{cand_semantic.get('mrr', 0.0):.3f} < {base_semantic.get('mrr', 0.0):.3f}"
        )
    if (cand_hybrid.get("mrr") or 0.0) + args.allow_hybrid_mrr_drop < (base_hybrid.get("mrr") or 0.0):
        failures.append(
            "get_ranked_context MRR regressed: "
            f"{cand_hybrid.get('mrr', 0.0):.3f} < {base_hybrid.get('mrr', 0.0):.3f}"
        )
    if (cand_hybrid.get("acc1") or 0.0) + args.allow_hybrid_acc1_drop < (base_hybrid.get("acc1") or 0.0):
        failures.append(
            "get_ranked_context Acc@1 regressed: "
            f"{cand_hybrid.get('acc1', 0.0):.3f} < {base_hybrid.get('acc1', 0.0):.3f}"
        )

    if baseline_paper and candidate_paper:
        base_source = baseline_paper["selected_cohort"]["source_kind"]
        cand_source = candidate_paper["selected_cohort"]["source_kind"]
        base_success = baseline_paper["harness_metrics"]["task_success_rate"]
        cand_success = candidate_paper["harness_metrics"]["task_success_rate"]
        if args.require_real_session_harness and (base_source != "real-session" or cand_source != "real-session"):
            failures.append(
                "real-session harness required, but selected cohort was "
                f"baseline={base_source}, candidate={cand_source}"
            )
        elif base_source != "real-session" or cand_source != "real-session":
            warnings.append(
                "Harness benchmark used synthetic-only cohort; task success is still checked, "
                "but this is not strong evidence for paper claims."
            )
        if base_success is not None and cand_success is not None:
            if cand_success + args.allow_task_success_drop < base_success:
                failures.append(
                    "task success rate regressed: "
                    f"{cand_success:.3f} < {base_success:.3f}"
                )

        base_tokens = baseline_paper["harness_metrics"]["tokens_per_successful_task"]
        cand_tokens = candidate_paper["harness_metrics"]["tokens_per_successful_task"]
        if base_tokens is not None and cand_tokens is not None:
            if args.max_token_per_success_increase >= 0 and cand_tokens > base_tokens + args.max_token_per_success_increase:
                failures.append(
                    "tokens per successful task increased beyond threshold: "
                    f"{cand_tokens:.1f} > {base_tokens:.1f} + {args.max_token_per_success_increase:.1f}"
                )
            elif cand_tokens > base_tokens:
                warnings.append(
                    "tokens per successful task increased: "
                    f"{cand_tokens:.1f} > {base_tokens:.1f}"
                )

        base_latency = baseline_paper["harness_metrics"]["latency_per_successful_task_ms"]
        cand_latency = candidate_paper["harness_metrics"]["latency_per_successful_task_ms"]
        if base_latency is not None and cand_latency is not None:
            if args.max_latency_per_success_increase_ms >= 0 and cand_latency > base_latency + args.max_latency_per_success_increase_ms:
                failures.append(
                    "latency per successful task increased beyond threshold: "
                    f"{cand_latency:.1f} > {base_latency:.1f} + {args.max_latency_per_success_increase_ms:.1f}"
                )
            elif cand_latency > base_latency:
                warnings.append(
                    "latency per successful task increased: "
                    f"{cand_latency:.1f} > {base_latency:.1f}"
                )
    elif not args.skip_harness:
        warnings.append("Harness comparison was skipped because paper benchmark inputs were unavailable.")

    deltas = {
        "semantic_search_mrr": (cand_semantic.get("mrr") or 0.0) - (base_semantic.get("mrr") or 0.0),
        "get_ranked_context_mrr": (cand_hybrid.get("mrr") or 0.0) - (base_hybrid.get("mrr") or 0.0),
        "get_ranked_context_acc1": (cand_hybrid.get("acc1") or 0.0) - (base_hybrid.get("acc1") or 0.0),
    }
    if baseline_paper and candidate_paper:
        for key in ("task_success_rate", "tokens_per_successful_task", "latency_per_successful_task_ms"):
            base_value = baseline_paper["harness_metrics"].get(key)
            cand_value = candidate_paper["harness_metrics"].get(key)
            deltas[key] = None if base_value is None or cand_value is None else cand_value - base_value
    return failures, warnings, deltas


def main():
    args = parse_args()
    output_dir = Path(args.output_dir).expanduser().resolve()
    output_dir.mkdir(parents=True, exist_ok=True)

    project = str(Path(args.project).expanduser().resolve())
    binary = str(Path(args.binary).expanduser().resolve())
    candidate_label = args.candidate_label or (
        Path(args.candidate_onnx_dir).resolve().parent.name if args.candidate_onnx_dir else "candidate"
    )

    cleanup_dirs: list[Path] = []
    candidate_env: dict[str, str] | None = None
    if args.candidate_onnx_dir and not args.candidate_retrieval_report:
        staged_root, candidate_env = stage_candidate_model(
            Path(args.candidate_onnx_dir).expanduser().resolve(),
            candidate_label,
        )
        cleanup_dirs.append(staged_root)
    elif args.candidate_onnx_dir and not args.candidate_harness_report and not args.skip_harness:
        staged_root, candidate_env = stage_candidate_model(
            Path(args.candidate_onnx_dir).expanduser().resolve(),
            candidate_label,
        )
        cleanup_dirs.append(staged_root)

    baseline_dir = output_dir / "baseline"
    candidate_dir = output_dir / candidate_label
    baseline_dir.mkdir(parents=True, exist_ok=True)
    candidate_dir.mkdir(parents=True, exist_ok=True)

    try:
        baseline_retrieval_path = Path(args.baseline_retrieval_report).expanduser().resolve() if args.baseline_retrieval_report else baseline_dir / "embedding-quality.json"
        candidate_retrieval_path = Path(args.candidate_retrieval_report).expanduser().resolve() if args.candidate_retrieval_report else candidate_dir / "embedding-quality.json"
        if not args.baseline_retrieval_report:
            run_retrieval_benchmark(
                project,
                binary,
                baseline_retrieval_path,
                baseline_dir / "embedding-quality.md",
                env=None,
            )
        if not args.candidate_retrieval_report:
            if not candidate_env:
                raise SystemExit("Need --candidate-onnx-dir or --candidate-retrieval-report")
            run_retrieval_benchmark(
                project,
                binary,
                candidate_retrieval_path,
                candidate_dir / "embedding-quality.md",
                env=candidate_env,
            )

        baseline_paper_path = None
        candidate_paper_path = None
        baseline_harness_path = None
        candidate_harness_path = None
        if not args.skip_harness:
            baseline_harness_path = Path(args.baseline_harness_report).expanduser().resolve() if args.baseline_harness_report else baseline_dir / "harness-eval.json"
            candidate_harness_path = Path(args.candidate_harness_report).expanduser().resolve() if args.candidate_harness_report else candidate_dir / "harness-eval.json"
            if not args.baseline_harness_report:
                run_harness_benchmark(
                    project,
                    binary,
                    baseline_harness_path,
                    baseline_dir / "harness-eval.md",
                    env=None,
                    include_real_sessions=args.include_real_sessions,
                )
            if not args.candidate_harness_report:
                if not candidate_env:
                    raise SystemExit("Need --candidate-onnx-dir or --candidate-harness-report")
                run_harness_benchmark(
                    project,
                    binary,
                    candidate_harness_path,
                    candidate_dir / "harness-eval.md",
                    env=candidate_env,
                    include_real_sessions=args.include_real_sessions,
                )
            baseline_paper_path = baseline_dir / "paper-benchmark.json"
            candidate_paper_path = candidate_dir / "paper-benchmark.json"
            run_paper_benchmark(
                baseline_harness_path,
                baseline_retrieval_path,
                baseline_paper_path,
                baseline_dir / "paper-benchmark.md",
            )
            run_paper_benchmark(
                candidate_harness_path,
                candidate_retrieval_path,
                candidate_paper_path,
                candidate_dir / "paper-benchmark.md",
            )

        baseline_retrieval = load_json(baseline_retrieval_path)
        candidate_retrieval = load_json(candidate_retrieval_path)
        baseline_paper = load_json(baseline_paper_path) if baseline_paper_path else None
        candidate_paper = load_json(candidate_paper_path) if candidate_paper_path else None
        failures, warnings, deltas = compare_metrics(
            args,
            baseline_retrieval,
            candidate_retrieval,
            baseline_paper,
            candidate_paper,
        )

        report = {
            "schema_version": "codelens-promotion-gate-v1",
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "candidate_label": candidate_label,
            "inputs": {
                "project": project,
                "binary": binary,
                "candidate_onnx_dir": args.candidate_onnx_dir or None,
                "baseline_retrieval_report": str(baseline_retrieval_path),
                "candidate_retrieval_report": str(candidate_retrieval_path),
                "baseline_harness_report": str(baseline_harness_path) if baseline_harness_path else None,
                "candidate_harness_report": str(candidate_harness_path) if candidate_harness_path else None,
                "baseline_paper_report": str(baseline_paper_path) if baseline_paper_path else None,
                "candidate_paper_report": str(candidate_paper_path) if candidate_paper_path else None,
            },
            "baseline": {
                "embedding_model": baseline_retrieval.get("embedding_model"),
                "semantic_search": method(baseline_retrieval, "semantic_search"),
                "get_ranked_context": method(baseline_retrieval, "get_ranked_context"),
                "paper_benchmark": baseline_paper,
            },
            "candidate": {
                "embedding_model": candidate_retrieval.get("embedding_model"),
                "semantic_search": method(candidate_retrieval, "semantic_search"),
                "get_ranked_context": method(candidate_retrieval, "get_ranked_context"),
                "paper_benchmark": candidate_paper,
            },
            "deltas": deltas,
            "warnings": warnings,
            "failures": failures,
            "passed": not failures,
        }
        report_path = output_dir / "promotion-gate-report.json"
        report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(report, ensure_ascii=False, indent=2))
        if failures:
            raise SystemExit(1)
    finally:
        for path in cleanup_dirs:
            shutil.rmtree(path, ignore_errors=True)


if __name__ == "__main__":
    main()
