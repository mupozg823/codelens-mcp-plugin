#!/usr/bin/env python3
"""Run the external cross-repo benchmark in three ablation arms.

Arms:
- bridge-off  : CODELENS_GENERIC_BRIDGES_OFF=1, no .codelens/bridges.json
- generic-on  : default generic bridges, no .codelens/bridges.json
- repo-on     : generic bridges + .codelens/bridges.json copied from the
                per-repo override (project-specific NL→code mappings)

For each repo x arm we invoke benchmarks/embedding-quality.py with the
right env and project path, capture the output JSON, and emit a matrix
of hybrid / semantic / lexical MRR per arm.

This script does NOT check out target repos. Point it at an already
prepared directory of worktrees::

    external-repos/
      axum/                         <- source tree
      ripgrep/
      django/
      typescript/
      bridges/
        axum.bridges.json           <- optional per-repo override
        ripgrep.bridges.json

A repo without an override has bridge-off and generic-on measured; the
repo-on arm is skipped (reported as "-").

Usage::

    cargo build --release --features semantic
    python3 benchmarks/external-3arm.py \\
        --repos-dir external-repos \\
        --datasets axum:benchmarks/embedding-quality-dataset-axum.json \\
                   ripgrep:benchmarks/embedding-quality-dataset-ripgrep.json \\
                   django:benchmarks/embedding-quality-dataset-django.json \\
                   typescript:benchmarks/embedding-quality-dataset-typescript.json \\
        --output benchmarks/external-3arm-results.json

Scaffold for P1-5 from the 2026-04-17 planning session. Intentionally
sequential and uncached; a nightly CI job is the long-term home for it.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ARMS = ("bridge-off", "generic-on", "repo-on")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--repos-dir",
        required=True,
        help="Directory containing repo worktrees + optional bridges/ overrides",
    )
    parser.add_argument(
        "--datasets",
        nargs="+",
        required=True,
        help="Pairs of repo_slug:dataset_path (e.g. axum:benchmarks/...axum.json)",
    )
    parser.add_argument(
        "--binary",
        default=os.environ.get(
            "CODELENS_BIN",
            str(
                Path(__file__).resolve().parent.parent
                / "target"
                / "release"
                / "codelens-mcp"
            ),
        ),
    )
    parser.add_argument(
        "--output",
        default="benchmarks/external-3arm-results.json",
    )
    return parser.parse_args()


def parse_datasets(specs: list[str]) -> list[tuple[str, Path]]:
    out: list[tuple[str, Path]] = []
    for spec in specs:
        if ":" not in spec:
            raise SystemExit(f"invalid --datasets entry (missing ':'): {spec}")
        slug, dataset_path = spec.split(":", 1)
        path = Path(dataset_path).resolve()
        if not path.is_file():
            raise SystemExit(f"dataset not found: {path}")
        out.append((slug, path))
    return out


def arm_env(arm: str) -> dict[str, str]:
    env = os.environ.copy()
    env.pop("CODELENS_GENERIC_BRIDGES_OFF", None)
    if arm == "bridge-off":
        env["CODELENS_GENERIC_BRIDGES_OFF"] = "1"
    return env


def prepare_bridges(repos_dir: Path, slug: str, arm: str) -> Path | None:
    """Install or remove .codelens/bridges.json in the target worktree.

    For bridge-off and generic-on arms we clear the file; for repo-on we
    copy the optional override at ``{repos_dir}/bridges/{slug}.bridges.json``
    into ``{repos_dir}/{slug}/.codelens/bridges.json``. Returns the
    override source path when copied, else None.
    """
    repo_path = repos_dir / slug
    target = repo_path / ".codelens" / "bridges.json"
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.exists():
        target.unlink()
    if arm != "repo-on":
        return None
    override = repos_dir / "bridges" / f"{slug}.bridges.json"
    if not override.is_file():
        return None
    shutil.copy2(override, target)
    return override


def run_arm(
    *,
    binary: Path,
    repo_path: Path,
    dataset: Path,
    arm: str,
) -> dict:
    cmd = [
        sys.executable,
        str(Path(__file__).resolve().parent / "embedding-quality.py"),
        str(repo_path),
        "--binary",
        str(binary),
        "--dataset",
        str(dataset),
        "--isolated-copy",
    ]
    proc = subprocess.run(
        cmd,
        env=arm_env(arm),
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        tail = (
            proc.stderr.strip().splitlines()[-1]
            if proc.stderr.strip()
            else f"exit {proc.returncode}"
        )
        return {"error": tail, "raw_stderr_tail": proc.stderr[-2000:]}
    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        return {"error": f"invalid JSON on stdout: {exc}"}


def summarize(result: dict) -> dict:
    methods = result.get("methods", {}) if isinstance(result, dict) else {}

    def mrr_of(method_key: str) -> float | None:
        m = methods.get(method_key, {})
        return m.get("mrr")

    return {
        "hybrid_mrr": mrr_of("get_ranked_context_hybrid"),
        "semantic_mrr": mrr_of("semantic_search"),
        "lexical_mrr": mrr_of("get_ranked_context_lexical"),
        "error": result.get("error"),
    }


def format_table(matrix: dict[str, dict[str, dict]]) -> str:
    header = "| Repo | Arm | Hybrid MRR | Semantic MRR | Lexical MRR |"
    sep = "|------|-----|-----------:|-------------:|------------:|"
    rows = [header, sep]
    for slug in sorted(matrix):
        for arm in ARMS:
            per = matrix[slug].get(arm, {})
            if per.get("error"):
                rows.append(f"| {slug} | {arm} | - | - | - | ({per['error']})")
                continue
            h = per.get("hybrid_mrr")
            s = per.get("semantic_mrr")
            ll = per.get("lexical_mrr")
            rows.append(
                f"| {slug} | {arm} | "
                f"{'-' if h is None else round(h, 3)} | "
                f"{'-' if s is None else round(s, 3)} | "
                f"{'-' if ll is None else round(ll, 3)} |"
            )
    return "\n".join(rows)


def main() -> int:
    args = parse_args()
    repos_dir = Path(args.repos_dir).resolve()
    if not repos_dir.is_dir():
        print(f"ERROR: --repos-dir not found: {repos_dir}", file=sys.stderr)
        return 2

    binary = Path(args.binary).resolve()
    if not binary.is_file():
        print(f"ERROR: binary not found: {binary}", file=sys.stderr)
        return 2

    datasets = parse_datasets(args.datasets)
    matrix: dict[str, dict[str, dict]] = {}

    for slug, dataset in datasets:
        repo_path = repos_dir / slug
        if not repo_path.is_dir():
            print(f"ERROR: repo worktree missing: {repo_path}", file=sys.stderr)
            return 2
        matrix.setdefault(slug, {})
        for arm in ARMS:
            override = prepare_bridges(repos_dir, slug, arm)
            if arm == "repo-on" and override is None:
                matrix[slug][arm] = {
                    "error": "no per-repo bridges.json override available"
                }
                continue
            print(f"=== {slug} / {arm} ===", flush=True)
            raw = run_arm(
                binary=binary,
                repo_path=repo_path,
                dataset=dataset,
                arm=arm,
            )
            matrix[slug][arm] = summarize(raw)

    out_path = Path(args.output).resolve()
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(matrix, indent=2))

    table = format_table(matrix)
    print("\n" + table)

    md_path = out_path.with_suffix(".md")
    md_path.write_text(f"# External 3-arm matrix\n\n{table}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
