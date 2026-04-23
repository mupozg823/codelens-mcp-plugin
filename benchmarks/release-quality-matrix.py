#!/usr/bin/env python3
"""Thin release quality matrix for baseline-vs-candidate CodeLens binaries."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


SCRIPT_DIR = Path(__file__).resolve().parent
ROOT = SCRIPT_DIR.parent
DEFAULT_BASELINE_DIR = Path("/tmp/codelens-baseline-v1.9.57")
DEFAULT_SUITES = (
    "embedding_quality",
    "external_retrieval",
    "role_retrieval",
    "embedding_runtime",
    "http_surface",
    "call_graph",
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("project_path", nargs="?", default=".")
    parser.add_argument("--baseline-tag", default="v1.9.57")
    parser.add_argument("--baseline-dir", default=str(DEFAULT_BASELINE_DIR))
    parser.add_argument("--candidate-binary", default=str(ROOT / "target" / "release" / "codelens-mcp"))
    parser.add_argument(
        "--output-dir",
        default=str(
            ROOT
            / "benchmarks"
            / "results"
            / f"{datetime.now(timezone.utc).strftime('%Y%m%d-%H%M%S')}-release-quality-matrix"
        ),
    )
    parser.add_argument(
        "--suites",
        default=",".join(DEFAULT_SUITES),
        help="Comma-separated suite names. Default runs the full matrix.",
    )
    parser.add_argument("--preset", default="balanced")
    parser.add_argument("--http-iterations", type=int, default=3)
    parser.add_argument("--skip-baseline-build", action="store_true")
    parser.add_argument("--skip-candidate-build", action="store_true")
    parser.add_argument("--timeout-seconds", type=int, default=1800)
    return parser.parse_args()


def run_command(
    *,
    name: str,
    argv: list[str],
    cwd: Path,
    output_dir: Path,
    timeout_seconds: int,
    env: dict[str, str] | None = None,
) -> dict:
    started = time.perf_counter()
    result = subprocess.run(
        argv,
        cwd=str(cwd),
        capture_output=True,
        text=True,
        timeout=timeout_seconds,
        check=False,
        env=env,
    )
    elapsed_ms = round((time.perf_counter() - started) * 1000, 2)
    (output_dir / f"{name}.stdout.txt").write_text(result.stdout, encoding="utf-8")
    (output_dir / f"{name}.stderr.txt").write_text(result.stderr, encoding="utf-8")
    return {
        "name": name,
        "command": argv,
        "cwd": str(cwd),
        "returncode": result.returncode,
        "elapsed_ms": elapsed_ms,
        "stdout_log": str(output_dir / f"{name}.stdout.txt"),
        "stderr_log": str(output_dir / f"{name}.stderr.txt"),
    }


def require_success(result: dict) -> None:
    if result.get("returncode") == 0:
        return
    raise SystemExit(
        f"{result['name']} failed with {result['returncode']} "
        f"(stderr={result['stderr_log']})"
    )


def ensure_baseline_worktree(tag: str, baseline_dir: Path) -> None:
    if baseline_dir.exists():
        if not (baseline_dir / "Cargo.toml").exists():
            raise SystemExit(f"baseline dir exists but is not a CodeLens checkout: {baseline_dir}")
        return
    baseline_dir.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["git", "worktree", "add", "--detach", str(baseline_dir), tag],
        cwd=str(ROOT),
        check=True,
    )


def build_release_binary(worktree: Path, name: str, output_dir: Path, timeout_seconds: int) -> Path:
    result = run_command(
        name=name,
        argv=["cargo", "build", "-p", "codelens-mcp", "--release", "--features", "http"],
        cwd=worktree,
        output_dir=output_dir,
        timeout_seconds=timeout_seconds,
    )
    require_success(result)
    binary = worktree / "target" / "release" / "codelens-mcp"
    if not binary.exists():
        raise SystemExit(f"release build did not produce binary: {binary}")
    return binary


def sha256_file(path: Path) -> str | None:
    if not path.exists():
        return None
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def git_output(args: list[str], cwd: Path = ROOT) -> str:
    result = subprocess.run(args, cwd=str(cwd), capture_output=True, text=True, check=False)
    return result.stdout.strip()


def git_metadata() -> dict:
    status = git_output(["git", "status", "--porcelain=v1"])
    return {
        "head": git_output(["git", "rev-parse", "HEAD"]),
        "branch": git_output(["git", "branch", "--show-current"]),
        "dirty": bool(status),
        "dirty_entry_count": len([line for line in status.splitlines() if line.strip()]),
    }


def model_metadata() -> dict:
    model_path = ROOT / "crates" / "codelens-engine" / "models" / "codesearch" / "model.onnx"
    return {
        "model_path": str(model_path),
        "sha256": sha256_file(model_path),
    }


def load_json(path: Path) -> dict:
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def safe_delta(baseline: Any, candidate: Any) -> float | None:
    if baseline is None or candidate is None:
        return None
    return round(float(candidate) - float(baseline), 6)


def safe_pct_delta(baseline: Any, candidate: Any) -> float | None:
    if baseline in {None, 0} or candidate is None:
        return None
    return round(((float(candidate) / float(baseline)) - 1.0) * 100.0, 2)


def methods_by_name(payload: dict) -> dict[str, dict]:
    return {
        row["method"]: row
        for row in payload.get("methods", [])
        if isinstance(row, dict) and isinstance(row.get("method"), str)
    }


def compare_method_metrics(baseline: dict, candidate: dict) -> dict:
    left = methods_by_name(baseline)
    right = methods_by_name(candidate)
    comparisons = {}
    for name in sorted(set(left) & set(right)):
        rows = {}
        for key in ("mrr", "acc1", "acc3", "acc5", "avg_elapsed_ms"):
            rows[key] = {
                "baseline": left[name].get(key),
                "candidate": right[name].get(key),
                "delta": safe_delta(left[name].get(key), right[name].get(key)),
                "pct_delta": safe_pct_delta(left[name].get(key), right[name].get(key)),
            }
        comparisons[name] = rows
    return comparisons


def compare_call_graph(baseline: dict, candidate: dict) -> dict:
    left = baseline.get("metrics", {})
    right = candidate.get("metrics", {})
    keys = (
        "edge_recall_at_k",
        "mrr_first_expected_edge",
        "avg_elapsed_ms",
        "p95_elapsed_ms",
        "unresolved_rate",
        "fallback_rate",
        "confidence_honesty_failure_count",
        "forbidden_high_confidence_failure_count",
    )
    return {
        key: {
            "baseline": left.get(key),
            "candidate": right.get(key),
            "delta": safe_delta(left.get(key), right.get(key)),
            "pct_delta": safe_pct_delta(left.get(key), right.get(key)),
        }
        for key in keys
    }


def compare_http_surface(payload: dict) -> dict:
    return {
        "summary": payload.get("summary", {}),
        "comparisons": payload.get("comparisons", []),
    }


def compare_embedding_runtime(baseline: dict, candidate: dict) -> dict:
    keys = (
        "semantic_search",
        "get_ranked_context",
        "onboard_project",
    )
    comparisons = {}
    for key in keys:
        left = baseline.get(key, {})
        right = candidate.get(key, {})
        if not isinstance(left, dict) or not isinstance(right, dict):
            continue
        metrics = ("elapsed_ms",) if key == "onboard_project" else (
            "avg_elapsed_ms",
            "p50_ms",
            "p95_ms",
            "cold_ms",
            "max_elapsed_ms",
        )
        comparisons[key] = {
            metric: {
                "baseline": left.get(metric),
                "candidate": right.get(metric),
                "delta": safe_delta(left.get(metric), right.get(metric)),
                "pct_delta": safe_pct_delta(left.get(metric), right.get(metric)),
            }
            for metric in metrics
        }
    return comparisons


def suite_commands(
    suite: str,
    *,
    project: Path,
    baseline_binary: Path,
    candidate_binary: Path,
    output_dir: Path,
    preset: str,
    http_iterations: int,
) -> tuple[list[dict], dict]:
    py = sys.executable
    if suite == "call_graph":
        dataset = SCRIPT_DIR / "call-graph-quality-dataset.json"
        return (
            [
                {
                    "label": "baseline",
                    "argv": [
                        py,
                        str(SCRIPT_DIR / "call-graph-quality.py"),
                        "--binary",
                        str(baseline_binary),
                        "--dataset",
                        str(dataset),
                        "--preset",
                        preset,
                        "--output",
                        str(output_dir / "call-graph-baseline.json"),
                        "--markdown-output",
                        str(output_dir / "call-graph-baseline.md"),
                        "--isolated-copy",
                    ],
                    "json": output_dir / "call-graph-baseline.json",
                },
                {
                    "label": "candidate",
                    "argv": [
                        py,
                        str(SCRIPT_DIR / "call-graph-quality.py"),
                        "--binary",
                        str(candidate_binary),
                        "--dataset",
                        str(dataset),
                        "--preset",
                        preset,
                        "--output",
                        str(output_dir / "call-graph-candidate.json"),
                        "--markdown-output",
                        str(output_dir / "call-graph-candidate.md"),
                        "--isolated-copy",
                    ],
                    "json": output_dir / "call-graph-candidate.json",
                },
            ],
            {"type": "pair", "compare": "call_graph"},
        )
    if suite == "embedding_quality":
        return pair_script_commands(
            suite,
            SCRIPT_DIR / "embedding-quality.py",
            project,
            baseline_binary,
            candidate_binary,
            output_dir,
            [
                "--dataset",
                str(SCRIPT_DIR / "embedding-quality-dataset-self.json"),
                "--preset",
                preset,
                "--isolated-copy",
            ],
        )
    if suite == "external_retrieval":
        return pair_script_commands(
            suite,
            SCRIPT_DIR / "external-retrieval.py",
            None,
            baseline_binary,
            candidate_binary,
            output_dir,
            [
                "--dataset",
                str(SCRIPT_DIR / "external-retrieval-dataset.json"),
                "--preset",
                preset,
                "--isolated-copy",
            ],
        )
    if suite == "role_retrieval":
        return pair_script_commands(
            suite,
            SCRIPT_DIR / "role-retrieval.py",
            project,
            baseline_binary,
            candidate_binary,
            output_dir,
            [
                "--dataset",
                str(SCRIPT_DIR / "role-retrieval-dataset.json"),
                "--preset",
                preset,
                "--isolated-copy",
            ],
        )
    if suite == "embedding_runtime":
        return pair_script_commands(
            suite,
            SCRIPT_DIR / "embedding-runtime.py",
            project,
            baseline_binary,
            candidate_binary,
            output_dir,
            [
                "--preset",
                preset,
                "--isolated-copy",
            ],
            include_markdown=False,
        )
    if suite == "http_surface":
        output_json = output_dir / "http-surface.json"
        return (
            [
                {
                    "label": "comparison",
                    "argv": [
                        py,
                        str(SCRIPT_DIR / "http-surface-benchmark.py"),
                        str(project),
                        "--baseline-binary",
                        str(baseline_binary),
                        "--candidate-binary",
                        str(candidate_binary),
                        "--iterations",
                        str(http_iterations),
                        "--output-json",
                        str(output_json),
                        "--markdown-output",
                        str(output_dir / "http-surface.md"),
                    ],
                    "json": output_json,
                }
            ],
            {"type": "single", "compare": "http_surface"},
        )
    raise SystemExit(f"unknown suite: {suite}")


def pair_script_commands(
    suite: str,
    script: Path,
    project: Path | None,
    baseline_binary: Path,
    candidate_binary: Path,
    output_dir: Path,
    extra_args: list[str],
    *,
    include_markdown: bool = True,
) -> tuple[list[dict], dict]:
    py = sys.executable
    commands = []
    for label, binary in (("baseline", baseline_binary), ("candidate", candidate_binary)):
        output_json = output_dir / f"{suite}-{label}.json"
        markdown = output_dir / f"{suite}-{label}.md"
        argv = [py, str(script)]
        if project is not None:
            argv.append(str(project))
        argv.extend(["--binary", str(binary), "--output", str(output_json)])
        if include_markdown:
            argv.extend(["--markdown-output", str(markdown)])
        argv.extend(extra_args)
        commands.append({"label": label, "argv": argv, "json": output_json})
    compare_type = "embedding_runtime" if suite == "embedding_runtime" else "methods"
    return commands, {"type": "pair", "compare": compare_type}


def compare_suite(suite: str, command_specs: list[dict], compare_spec: dict) -> dict:
    if compare_spec["type"] == "single":
        payload = load_json(command_specs[0]["json"])
        return compare_http_surface(payload)
    baseline = load_json(next(spec["json"] for spec in command_specs if spec["label"] == "baseline"))
    candidate = load_json(next(spec["json"] for spec in command_specs if spec["label"] == "candidate"))
    if compare_spec["compare"] == "call_graph":
        return compare_call_graph(baseline, candidate)
    if compare_spec["compare"] == "embedding_runtime":
        return compare_embedding_runtime(baseline, candidate)
    if compare_spec["compare"] == "methods":
        return compare_method_metrics(baseline, candidate)
    raise SystemExit(f"unsupported compare type for {suite}: {compare_spec['compare']}")


def gate_results(comparisons: dict) -> list[str]:
    failures = []
    call_graph = comparisons.get("call_graph", {})
    if call_graph:
        recall = call_graph.get("edge_recall_at_k", {})
        if numeric_less(recall.get("candidate"), recall.get("baseline")):
            failures.append("call_graph edge_recall_at_k regressed")
        for key in (
            "confidence_honesty_failure_count",
            "forbidden_high_confidence_failure_count",
        ):
            if (call_graph.get(key, {}).get("candidate") or 0) > 0:
                failures.append(f"call_graph {key} is non-zero")
        p95 = call_graph.get("p95_elapsed_ms", {})
        if pct_worse_than(p95.get("baseline"), p95.get("candidate"), 25.0):
            failures.append("call_graph p95 latency regressed by more than 25%")

    external = comparisons.get("external_retrieval", {})
    ranked = external.get("get_ranked_context", {})
    mrr = ranked.get("mrr", {})
    if ranked and numeric_less(mrr.get("candidate"), mrr.get("baseline")):
        failures.append("external_retrieval get_ranked_context MRR regressed")
    latency = ranked.get("avg_elapsed_ms", {})
    if ranked and pct_worse_than(latency.get("baseline"), latency.get("candidate"), 25.0):
        failures.append("external_retrieval get_ranked_context latency regressed by more than 25%")

    return failures


def numeric_less(candidate: Any, baseline: Any) -> bool:
    if candidate is None or baseline is None:
        return False
    return float(candidate) < float(baseline)


def pct_worse_than(baseline: Any, candidate: Any, threshold_pct: float) -> bool:
    pct = safe_pct_delta(baseline, candidate)
    return pct is not None and pct > threshold_pct


def render_markdown(report: dict) -> str:
    lines = [
        "# Release Quality Matrix",
        "",
        f"- Baseline tag: `{report['baseline_tag']}`",
        f"- Baseline binary: `{report['baseline_binary']['path']}`",
        f"- Candidate binary: `{report['candidate_binary']['path']}`",
        f"- Candidate git dirty: `{report['git']['dirty']}` ({report['git']['dirty_entry_count']} entries)",
        f"- Suites: `{', '.join(report['suites'])}`",
        f"- Gate passed: `{report['gate']['passed']}`",
        "",
    ]
    if report["gate"]["failures"]:
        lines.append("## Gate Failures")
        lines.append("")
        for failure in report["gate"]["failures"]:
            lines.append(f"- {failure}")
        lines.append("")
    lines.extend(["## Artifacts", ""])
    for suite, artifact in report.get("artifacts", {}).items():
        lines.append(f"- `{suite}`: {artifact.get('status')}")
    lines.append("")
    return "\n".join(lines)


def main() -> None:
    args = parse_args()
    project = Path(args.project_path).expanduser().resolve()
    output_dir = Path(args.output_dir).expanduser().resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    baseline_dir = Path(args.baseline_dir).expanduser().resolve()
    suites = [suite.strip() for suite in args.suites.split(",") if suite.strip()]

    ensure_baseline_worktree(args.baseline_tag, baseline_dir)
    if args.skip_baseline_build:
        baseline_binary = baseline_dir / "target" / "release" / "codelens-mcp"
        if not baseline_binary.exists():
            raise SystemExit(f"missing baseline binary under --skip-baseline-build: {baseline_binary}")
    else:
        baseline_binary = build_release_binary(
            baseline_dir,
            "build-baseline",
            output_dir,
            args.timeout_seconds,
        )

    default_candidate_binary = (ROOT / "target" / "release" / "codelens-mcp").resolve()
    candidate_binary = Path(args.candidate_binary).expanduser().resolve()
    if not args.skip_candidate_build and candidate_binary == default_candidate_binary:
        candidate_binary = build_release_binary(
            ROOT,
            "build-candidate",
            output_dir,
            args.timeout_seconds,
        )
    elif not args.skip_candidate_build and not candidate_binary.exists():
        raise SystemExit(
            "candidate binary does not exist and cannot be auto-built outside the default "
            f"target path: {candidate_binary}"
        )
    if not candidate_binary.exists():
        raise SystemExit(f"candidate binary does not exist: {candidate_binary}")

    env = os.environ.copy()
    model_dir = ROOT / "crates" / "codelens-engine" / "models"
    if (model_dir / "codesearch" / "model.onnx").exists():
        env.setdefault("CODELENS_MODEL_DIR", str(model_dir))

    artifacts = {}
    comparisons = {}
    for suite in suites:
        suite_dir = output_dir / suite
        suite_dir.mkdir(parents=True, exist_ok=True)
        command_specs, compare_spec = suite_commands(
            suite,
            project=project,
            baseline_binary=baseline_binary,
            candidate_binary=candidate_binary,
            output_dir=suite_dir,
            preset=args.preset,
            http_iterations=args.http_iterations,
        )
        command_results = []
        for spec in command_specs:
            result = run_command(
                name=f"{suite}-{spec['label']}",
                argv=spec["argv"],
                cwd=ROOT,
                output_dir=suite_dir,
                timeout_seconds=args.timeout_seconds,
                env=env,
            )
            command_results.append(result)
            require_success(result)
        comparisons[suite] = compare_suite(suite, command_specs, compare_spec)
        artifacts[suite] = {
            "status": "completed",
            "commands": command_results,
            "json_outputs": [str(spec["json"]) for spec in command_specs],
        }

    gate_failures = gate_results(comparisons)
    report = {
        "schema_version": "codelens-release-quality-matrix-v1",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "project": str(project),
        "baseline_tag": args.baseline_tag,
        "baseline_worktree": str(baseline_dir),
        "baseline_binary": {
            "path": str(baseline_binary),
            "sha256": sha256_file(baseline_binary),
        },
        "candidate_binary": {
            "path": str(candidate_binary),
            "sha256": sha256_file(candidate_binary),
        },
        "git": git_metadata(),
        "model": model_metadata(),
        "suites": suites,
        "artifacts": artifacts,
        "comparisons": comparisons,
        "gate": {
            "passed": not gate_failures,
            "failures": gate_failures,
        },
    }
    summary_json = output_dir / "summary.json"
    summary_md = output_dir / "summary.md"
    summary_json.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    summary_md.write_text(render_markdown(report), encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False, indent=2))
    if gate_failures:
        raise SystemExit(2)


if __name__ == "__main__":
    main()
