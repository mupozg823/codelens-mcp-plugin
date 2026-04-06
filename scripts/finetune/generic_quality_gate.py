#!/usr/bin/env python3
"""Run a generic embedding quality gate for a candidate model.

The gate compares a new model against existing benchmark baselines using:
  1. embedding-quality.py
  2. multi-repo-eval.py

It exits non-zero on regression beyond configured tolerances.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).parent.parent.parent


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--onnx-dir", required=True, help="Directory containing model.onnx + tokenizer files")
    parser.add_argument("--binary", default=os.environ.get("CODELENS_BIN", str(ROOT / "target" / "debug" / "codelens-mcp")))
    parser.add_argument("--output-dir", default=str(Path(__file__).parent / "gate-results"))
    parser.add_argument(
        "--embedding-baseline",
        default=str(ROOT / "benchmarks" / "embedding-quality-results-v5.json"),
    )
    parser.add_argument(
        "--multi-repo-baseline",
        default=str(ROOT / "benchmarks" / "eval-results" / "summary.json"),
    )
    parser.add_argument("--repos", default=str(ROOT / "benchmarks" / "eval-repos.json"))
    parser.add_argument("--max-repos", type=int, default=5)
    parser.add_argument("--allow-semantic-mrr-drop", type=float, default=0.0)
    parser.add_argument("--allow-hybrid-mrr-drop", type=float, default=0.0)
    parser.add_argument("--allow-multi-repo-drop", type=float, default=0.0)
    return parser.parse_args()


def run(cmd: list[str], env: dict[str, str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, cwd=ROOT, env=env, text=True, capture_output=True, check=False)


def load_json(path: Path) -> dict:
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def embedding_method(result: dict, name: str) -> dict:
    for method in result.get("methods", []):
        if method.get("method") == name:
            return method
    return {}


def main():
    args = parse_args()
    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    onnx_dir = Path(args.onnx_dir).resolve()
    if not onnx_dir.exists():
        raise SystemExit(f"ONNX dir not found: {onnx_dir}")

    staging_root = Path(tempfile.mkdtemp(prefix="codelens-gate-"))
    model_root = staging_root / "codesearch"
    shutil.copytree(onnx_dir, model_root)

    env = os.environ.copy()
    env["CODELENS_MODEL_DIR"] = str(staging_root)

    embedding_output = output_dir / "embedding-quality.json"
    multi_repo_output = output_dir / "multi-repo"

    embedding_cmd = [
        "python3",
        "benchmarks/embedding-quality.py",
        ".",
        "--binary",
        args.binary,
        "--output",
        str(embedding_output),
    ]
    multi_repo_cmd = [
        "python3",
        "benchmarks/multi-repo-eval.py",
        "--binary",
        args.binary,
        "--repos",
        args.repos,
        "--output",
        str(multi_repo_output),
        "--max-repos",
        str(args.max_repos),
    ]

    embedding_run = run(embedding_cmd, env)
    if embedding_run.returncode != 0:
        raise SystemExit(
            "embedding-quality.py failed\n"
            f"stdout:\n{embedding_run.stdout}\n\nstderr:\n{embedding_run.stderr}"
        )

    multi_repo_run = run(multi_repo_cmd, env)
    if multi_repo_run.returncode != 0:
        raise SystemExit(
            "multi-repo-eval.py failed\n"
            f"stdout:\n{multi_repo_run.stdout}\n\nstderr:\n{multi_repo_run.stderr}"
        )

    current_embedding = load_json(embedding_output)
    baseline_embedding = load_json(Path(args.embedding_baseline))
    current_multi = load_json(multi_repo_output / "summary.json")
    baseline_multi = load_json(Path(args.multi_repo_baseline))

    current_semantic = embedding_method(current_embedding, "semantic_search").get("mrr", 0.0)
    baseline_semantic = embedding_method(baseline_embedding, "semantic_search").get("mrr", 0.0)
    current_hybrid = embedding_method(current_embedding, "get_ranked_context").get("mrr", 0.0)
    baseline_hybrid = embedding_method(baseline_embedding, "get_ranked_context").get("mrr", 0.0)
    current_multi_semantic = current_multi.get("avg_semantic_mrr", 0.0)
    baseline_multi_semantic = baseline_multi.get("avg_semantic_mrr", 0.0)

    failures = []
    if current_semantic + args.allow_semantic_mrr_drop < baseline_semantic:
        failures.append(
            f"semantic_search MRR regressed: {current_semantic:.3f} < {baseline_semantic:.3f}"
        )
    if current_hybrid + args.allow_hybrid_mrr_drop < baseline_hybrid:
        failures.append(
            f"get_ranked_context MRR regressed: {current_hybrid:.3f} < {baseline_hybrid:.3f}"
        )
    if baseline_multi and current_multi_semantic + args.allow_multi_repo_drop < baseline_multi_semantic:
        failures.append(
            f"multi-repo semantic MRR regressed: {current_multi_semantic:.3f} < {baseline_multi_semantic:.3f}"
        )

    report = {
        "candidate_model_dir": str(onnx_dir),
        "embedding_quality_output": str(embedding_output),
        "multi_repo_output": str(multi_repo_output),
        "current": {
            "semantic_search_mrr": current_semantic,
            "hybrid_mrr": current_hybrid,
            "multi_repo_semantic_mrr": current_multi_semantic,
        },
        "baseline": {
            "semantic_search_mrr": baseline_semantic,
            "hybrid_mrr": baseline_hybrid,
            "multi_repo_semantic_mrr": baseline_multi_semantic,
        },
        "failures": failures,
        "passed": not failures,
    }
    report_path = output_dir / "gate-report.json"
    report_path.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    print(json.dumps(report, indent=2, ensure_ascii=False))
    if failures:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
