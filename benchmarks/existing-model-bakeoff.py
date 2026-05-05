#!/usr/bin/env python3
"""Run embedding-quality.py across locally available ONNX model directories."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
from datetime import datetime, timezone
from pathlib import Path
from typing import NamedTuple

from benchmark_runtime_common import REQUIRED_MODEL_ASSETS, model_dir_has_assets


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT_DIR = Path("/tmp/codelens-existing-model-bakeoff")
DEFAULT_CANDIDATE_PATHS = (
    ("bundled", "crates/codelens-engine/models/codesearch"),
    ("v6-internet", "scripts/finetune/output/v6-internet/onnx"),
    ("v7-nl-augmented", "scripts/finetune/output/v7-nl-augmented/onnx"),
    ("v8-final", "scripts/finetune/output/v8-final/onnx"),
)


class Candidate(NamedTuple):
    label: str
    model_dir: Path


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("project_path", nargs="?", default=".")
    parser.add_argument(
        "--binary",
        default=os.environ.get(
            "CODELENS_BIN", str(REPO_ROOT / "target" / "debug" / "codelens-mcp")
        ),
    )
    parser.add_argument(
        "--dataset",
        default=str(REPO_ROOT / "benchmarks" / "embedding-quality-dataset-self.json"),
    )
    parser.add_argument("--output-dir", default=str(DEFAULT_OUTPUT_DIR))
    parser.add_argument("--summary-json", default="")
    parser.add_argument("--summary-md", default="")
    parser.add_argument("--preset", default="balanced")
    parser.add_argument("--max-results", type=int, default=10)
    parser.add_argument("--ranked-context-max-tokens", type=int, default=50000)
    parser.add_argument(
        "--candidate",
        action="append",
        default=[],
        metavar="LABEL=DIR",
        help="Candidate model dir. Can be passed multiple times.",
    )
    parser.add_argument(
        "--reuse-project-index",
        dest="isolated_copy",
        action="store_false",
        help="Reuse the project working tree and existing .codelens index. Not recommended for model comparisons.",
    )
    parser.set_defaults(isolated_copy=True)
    parser.add_argument(
        "--set-embed-model-label",
        action="store_true",
        help="Also pass candidate label through CODELENS_EMBED_MODEL. Requires a binary built with model-bakeoff for non-default labels.",
    )
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--fail-fast", action="store_true")
    return parser.parse_args(argv)


def parse_candidate_spec(value: str) -> Candidate:
    if "=" not in value:
        raise SystemExit("--candidate must use LABEL=DIR")
    label, path = value.split("=", 1)
    label = label.strip()
    path = path.strip()
    if not label or not path:
        raise SystemExit("--candidate must include both LABEL and DIR")
    return Candidate(label=label, model_dir=Path(path))


def discover_default_candidates(repo_root: Path = REPO_ROOT) -> list[Candidate]:
    candidates = []
    for label, relative_path in DEFAULT_CANDIDATE_PATHS:
        model_dir = repo_root / relative_path
        if model_dir_has_assets(model_dir):
            candidates.append(Candidate(label=label, model_dir=model_dir))
    return candidates


def candidate_report_dir(output_dir: Path, label: str) -> Path:
    safe = "".join(ch if ch.isalnum() or ch in "._-" else "-" for ch in label)
    return output_dir / safe


def embedding_quality_command(
    args: argparse.Namespace,
    candidate: Candidate,
    report_dir: Path,
) -> list[str]:
    cmd = [
        "python3",
        "benchmarks/embedding-quality.py",
        args.project_path,
        "--binary",
        str(Path(args.binary)),
        "--dataset",
        str(Path(args.dataset)),
        "--preset",
        args.preset,
        "--max-results",
        str(args.max_results),
        "--ranked-context-max-tokens",
        str(args.ranked_context_max_tokens),
        "--output",
        str(report_dir / "embedding-quality.json"),
        "--markdown-output",
        str(report_dir / "embedding-quality.md"),
    ]
    if args.isolated_copy:
        cmd.append("--isolated-copy")
    if args.set_embed_model_label:
        cmd.extend(["--embed-model", candidate.label])
    return cmd


def method_by_name(report: dict, name: str) -> dict:
    for method in report.get("methods", []):
        if method.get("method") == name:
            return method
    raise SystemExit(f"report missing method: {name}")


def summarize_report(candidate: Candidate, report_path: Path) -> dict:
    report = json.loads(report_path.read_text(encoding="utf-8"))
    semantic = method_by_name(report, "semantic_search")
    lexical = method_by_name(report, "get_ranked_context_no_semantic")
    hybrid = method_by_name(report, "get_ranked_context")
    return {
        "label": candidate.label,
        "model_dir": str(candidate.model_dir),
        "status": "ok",
        "report_path": str(report_path),
        "markdown_path": str(report_path.with_suffix(".md")),
        "dataset_size": report.get("dataset_size"),
        "runtime_model": report.get("runtime_model"),
        "semantic_mrr": semantic.get("mrr"),
        "semantic_acc1": semantic.get("acc1"),
        "lexical_mrr": lexical.get("mrr"),
        "hybrid_mrr": hybrid.get("mrr"),
        "hybrid_acc1": hybrid.get("acc1"),
        "hybrid_acc3": hybrid.get("acc3"),
        "hybrid_acc5": hybrid.get("acc5"),
        "hybrid_avg_elapsed_ms": hybrid.get("avg_elapsed_ms"),
        "hybrid_uplift": report.get("hybrid_uplift"),
    }


def run_candidate(args: argparse.Namespace, candidate: Candidate, output_dir: Path) -> dict:
    report_dir = candidate_report_dir(output_dir, candidate.label)
    report_dir.mkdir(parents=True, exist_ok=True)
    report_path = report_dir / "embedding-quality.json"
    cmd = embedding_quality_command(args, candidate, report_dir)
    env = os.environ.copy()
    env["CODELENS_MODEL_DIR"] = str(candidate.model_dir.resolve())
    if args.set_embed_model_label:
        env["CODELENS_EMBED_MODEL"] = candidate.label
    else:
        env.pop("CODELENS_EMBED_MODEL", None)

    if args.dry_run:
        return {
            "label": candidate.label,
            "model_dir": str(candidate.model_dir),
            "status": "dry_run",
            "command": cmd,
        }

    result = subprocess.run(
        cmd,
        cwd=REPO_ROOT,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        return {
            "label": candidate.label,
            "model_dir": str(candidate.model_dir),
            "status": "failed",
            "returncode": result.returncode,
            "stdout_tail": result.stdout[-4000:],
            "stderr_tail": result.stderr[-4000:],
            "command": cmd,
        }
    return summarize_report(candidate, report_path)


def render_markdown(summary: dict) -> str:
    lines = [
        "# Existing Model Bakeoff",
        "",
        f"- Dataset: `{summary['dataset']}`",
        f"- Binary: `{summary['binary']}`",
        f"- Ranked context max tokens: `{summary['ranked_context_max_tokens']}`",
        f"- Isolated copy: `{summary['isolated_copy']}`",
        "",
        "| Model | Status | Hybrid MRR | Semantic MRR | Lexical MRR | Acc@1 | Acc@3 | Acc@5 | Avg ms | SHA256 |",
        "|---|---|---:|---:|---:|---:|---:|---:|---:|---|",
    ]
    for item in summary["leaderboard"]:
        runtime_model = item.get("runtime_model") or {}
        sha = str(runtime_model.get("sha256") or "")[:12]
        lines.append(
            "| {label} | {status} | {hybrid_mrr:.3f} | {semantic_mrr:.3f} | "
            "{lexical_mrr:.3f} | {hybrid_acc1:.0%} | {hybrid_acc3:.0%} | "
            "{hybrid_acc5:.0%} | {hybrid_avg_elapsed_ms:.1f} | `{sha}` |".format(
                sha=sha,
                **item,
            )
        )
    failed = [item for item in summary["candidates"] if item.get("status") == "failed"]
    if failed:
        lines.extend(["", "## Failed Candidates", ""])
        for item in failed:
            lines.append(f"- `{item['label']}`: returncode={item.get('returncode')}")
    lines.append("")
    return "\n".join(lines)


def build_summary(args: argparse.Namespace, candidates: list[Candidate], results: list[dict]) -> dict:
    ok = [item for item in results if item.get("status") == "ok"]
    leaderboard = sorted(
        ok,
        key=lambda item: (
            item.get("hybrid_mrr") or 0.0,
            item.get("semantic_mrr") or 0.0,
        ),
        reverse=True,
    )
    return {
        "schema_version": "codelens-existing-model-bakeoff-v1",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "project": str(Path(args.project_path).resolve()),
        "binary": str(Path(args.binary).resolve()),
        "dataset": str(Path(args.dataset).resolve()),
        "ranked_context_max_tokens": args.ranked_context_max_tokens,
        "max_results": args.max_results,
        "isolated_copy": args.isolated_copy,
        "candidate_count": len(candidates),
        "candidates": results,
        "leaderboard": leaderboard,
        "best": leaderboard[0] if leaderboard else None,
    }


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    output_dir = Path(args.output_dir)
    candidates = (
        [parse_candidate_spec(value) for value in args.candidate]
        if args.candidate
        else discover_default_candidates(REPO_ROOT)
    )
    if not candidates:
        raise SystemExit("No complete local model candidates found")

    results = []
    for candidate in candidates:
        if not model_dir_has_assets(candidate.model_dir):
            results.append(
                {
                    "label": candidate.label,
                    "model_dir": str(candidate.model_dir),
                    "status": "missing_assets",
                }
            )
            continue
        result = run_candidate(args, candidate, output_dir)
        results.append(result)
        if args.fail_fast and result.get("status") == "failed":
            break

    summary = build_summary(args, candidates, results)
    summary_json = Path(args.summary_json) if args.summary_json else output_dir / "summary.json"
    summary_md = Path(args.summary_md) if args.summary_md else output_dir / "summary.md"
    summary_json.parent.mkdir(parents=True, exist_ok=True)
    summary_json.write_text(
        json.dumps(summary, indent=2, ensure_ascii=False, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    summary_md.write_text(render_markdown(summary), encoding="utf-8")
    print(json.dumps(summary, indent=2, ensure_ascii=False, sort_keys=True))
    return 1 if any(item.get("status") == "failed" for item in results) else 0


if __name__ == "__main__":
    raise SystemExit(main())
