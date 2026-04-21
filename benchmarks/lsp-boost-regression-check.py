#!/usr/bin/env python3
"""P1-5: regression gate for the `lsp_boost` caller-wiring feature.

Runs the three-repo stratified matrix (self / flask / zod) and
compares the `thick_caller` bucket hit rate and MRR against a pinned
contract. Fails with a non-zero exit code when any row drops below
the contract, so a CI job or pre-merge script can block changes to
`crates/codelens-engine/src/symbols/ranking.rs`,
`crates/codelens-mcp/src/tools/symbols/handlers.rs`, or
`crates/codelens-engine/src/lsp/**` that quietly regress the feature.

The contract is hard-coded from the v1.9.50 post-rescue, post-wait-
for-ready matrix (see
`benchmarks/results/v1.9.50-lsp-boost-ts-interface-rescue.md` and
`benchmarks/results/v1.9.50-lsp-readiness-wait.md`). Thin-wrapper
rows are intentionally *not* gated — per-ref weighting's contract is
to refuse single-ref callers on dense-mention files, so the thin
bucket drifts by design on some repos.

Usage:
    # Run the full matrix (requires external-repos/flask, external-repos/zod):
    python3 benchmarks/lsp-boost-regression-check.py

    # Or replay a pre-run JSON set:
    python3 benchmarks/lsp-boost-regression-check.py \\
        --current-self <self.json> \\
        --current-flask <flask.json> \\
        --current-zod  <zod.json>

Exit codes:
    0 — every repo meets contract
    1 — at least one regression
    2 — runner error (missing binary, worktree, etc.)
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent


@dataclass(frozen=True)
class Contract:
    """Minimum acceptable thick-bucket state per repo.

    - `thick_hits`: absolute hit count — `must be >= thick_total`
      (i.e. every thick_caller row in the dataset must still surface
      its expected symbol). This is the primary regression guard.
    - `thick_mrr_min`: MRR floor. Allows some noise while still
      failing on catastrophic rank drops.
    - `thick_total`: expected dataset size for the bucket (used to
      sanity-check the bench output, not part of the contract per se).
    """

    thick_hits: int
    thick_total: int
    thick_mrr_min: float


# Contract pinned to v1.9.50 post-rescue numbers. If the numbers
# genuinely improve and you want to lock in the new floor, bump these
# along with the `benchmarks/results/v1.9.50-lsp-boost-*.md` docs.
CONTRACT: dict[str, Contract] = {
    "self": Contract(thick_hits=5, thick_total=5, thick_mrr_min=0.26),
    "flask": Contract(thick_hits=3, thick_total=3, thick_mrr_min=0.07),
    "zod": Contract(thick_hits=3, thick_total=3, thick_mrr_min=0.13),
}


REPO_DATASETS: dict[str, tuple[str, str]] = {
    "self": (".", "benchmarks/lsp-boost-dataset-self.json"),
    "flask": (
        "external-repos/flask",
        "benchmarks/lsp-boost-dataset-flask.json",
    ),
    "zod": (
        "external-repos/zod",
        "benchmarks/lsp-boost-dataset-zod.json",
    ),
}


# Grace / dead-lsp knobs mirror what the matrix docs used when the
# contract was pinned. Tight enough to shake out regressions, loose
# enough that healthy runs never hit the timeout.
REPO_TIMINGS: dict[str, dict[str, int]] = {
    "self": {"grace": 15, "dead_lsp": 20, "wait": 90},
    "flask": {"grace": 5, "dead_lsp": 20, "wait": 60},
    "zod": {"grace": 10, "dead_lsp": 5, "wait": 60},
}


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--binary",
        default=str(REPO_ROOT / "target" / "release" / "codelens-mcp"),
        help="Path to codelens-mcp release binary.",
    )
    p.add_argument(
        "--output-dir",
        default=str(REPO_ROOT / "benchmarks" / "results" / "regression"),
        help="Where to drop the per-repo matrix JSONs when running live.",
    )
    for repo in REPO_DATASETS:
        p.add_argument(
            f"--current-{repo}",
            default=None,
            help=f"Pre-run matrix JSON for {repo} (skips the live matrix "
            "invocation for this repo).",
        )
    p.add_argument(
        "--summary",
        default=None,
        help="Optional path to write the markdown summary to. Always "
        "prints to stdout regardless.",
    )
    return p.parse_args()


def run_matrix_for_repo(
    repo: str,
    *,
    binary: Path,
    output_dir: Path,
) -> Path:
    """Invoke `lsp-boost-http-matrix.py` for a single repo. Returns
    the path to the emitted JSON. Propagates subprocess failures as
    SystemExit(2)."""

    project_rel, dataset_rel = REPO_DATASETS[repo]
    project = REPO_ROOT / project_rel
    dataset = REPO_ROOT / dataset_rel
    if not project.is_dir():
        raise SystemExit(
            f"[{repo}] project worktree not found: {project}. "
            f"Clone it under external-repos/ first or pass "
            f"--current-{repo} to replay a pre-run JSON."
        )
    if not dataset.is_file():
        raise SystemExit(f"[{repo}] dataset missing: {dataset}")

    output_dir.mkdir(parents=True, exist_ok=True)
    output = output_dir / f"{repo}.json"
    timings = REPO_TIMINGS[repo]
    cmd = [
        sys.executable,
        str(REPO_ROOT / "benchmarks" / "lsp-boost-http-matrix.py"),
        "--binary",
        str(binary),
        "--project",
        str(project),
        "--dataset",
        str(dataset),
        "--output",
        str(output),
        "--auto-attach",
        "--auto-attach-wait",
        str(timings["wait"]),
        "--wait-for-ready",
        "--ready-grace-seconds",
        str(timings["grace"]),
        "--dead-lsp-timeout",
        str(timings["dead_lsp"]),
    ]
    print(f"[{repo}] running {' '.join(cmd)}", flush=True)
    env = os.environ.copy()
    proc = subprocess.run(cmd, env=env, check=False)
    if proc.returncode != 0:
        raise SystemExit(f"[{repo}] matrix runner exited {proc.returncode}")
    return output


def load_thick_bucket(path: Path) -> dict:
    """Extract the `lsp_boost` arm's `thick_caller` bucket summary
    from a matrix JSON. Returns an empty dict if the shape does not
    match (old schema, partial run, etc.)."""

    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except Exception as exc:
        raise SystemExit(f"failed to parse {path}: {exc}")
    arms = payload.get("arms")
    if not isinstance(arms, list):
        return {}
    for arm in arms:
        if arm.get("arm") != "lsp_boost":
            continue
        buckets = arm.get("by_bucket") or {}
        thick = buckets.get("thick_caller") or {}
        return {
            "hits": int(thick.get("hits") or 0),
            "count": int(thick.get("count") or 0),
            "mrr": float(thick.get("mrr") or 0.0),
            "total_mrr": float(arm.get("mrr") or 0.0),
            "total_hits": int(arm.get("hits") or 0),
        }
    return {}


def evaluate(repo: str, thick: dict, contract: Contract) -> list[str]:
    """Return a list of violation strings for this repo. Empty list
    means the row passes. The checks are intentionally conservative:
    we do not re-gate absolute counts when the dataset itself grew
    (someone might have added a new thick entry)."""

    violations: list[str] = []
    if not thick:
        violations.append("no thick_caller bucket found in lsp_boost arm")
        return violations
    if thick.get("count", 0) < contract.thick_total:
        violations.append(
            f"thick_caller count={thick['count']} < expected "
            f"{contract.thick_total} (did the dataset shrink?)"
        )
    if thick.get("hits", 0) < contract.thick_hits:
        violations.append(
            f"thick_caller hits={thick['hits']} < contract "
            f"{contract.thick_hits}/{contract.thick_total}"
        )
    if thick.get("mrr", 0.0) < contract.thick_mrr_min:
        violations.append(
            f"thick_caller MRR={thick['mrr']:.4f} < contract "
            f"{contract.thick_mrr_min:.4f}"
        )
    return violations


def format_summary(rows: list[dict]) -> str:
    header = (
        "| Repo  | Thick hits (contract) | Thick MRR (contract) | Total MRR | Status |"
    )
    sep = (
        "| ----- | --------------------- | -------------------- | --------- | ------ |"
    )
    lines = [header, sep]
    for row in rows:
        repo = row["repo"]
        thick = row["thick"]
        contract = CONTRACT[repo]
        hits_cell = (
            f"{thick.get('hits', 0)}/{thick.get('count', 0)} "
            f"({contract.thick_hits}/{contract.thick_total})"
        )
        mrr_cell = f"{thick.get('mrr', 0.0):.4f} (≥ {contract.thick_mrr_min:.4f})"
        status = "PASS" if not row["violations"] else "FAIL"
        total_mrr = thick.get("total_mrr", 0.0)
        lines.append(
            f"| {repo:5s} | {hits_cell:21s} | {mrr_cell:20s} | "
            f"{total_mrr:.4f}    | {status}   |"
        )
    return "\n".join(lines)


def main() -> int:
    args = parse_args()
    binary = Path(args.binary).resolve()
    output_dir = Path(args.output_dir).resolve()

    rows: list[dict] = []
    any_violations = False
    for repo in REPO_DATASETS:
        override = getattr(args, f"current_{repo}")
        if override is not None:
            result_path = Path(override).resolve()
            if not result_path.is_file():
                print(
                    f"[{repo}] --current-{repo} file missing: {result_path}",
                    file=sys.stderr,
                )
                return 2
        else:
            if not binary.is_file():
                print(f"ERROR: binary not found: {binary}", file=sys.stderr)
                return 2
            result_path = run_matrix_for_repo(
                repo, binary=binary, output_dir=output_dir
            )
        thick = load_thick_bucket(result_path)
        contract = CONTRACT[repo]
        violations = evaluate(repo, thick, contract)
        if violations:
            any_violations = True
        rows.append(
            {
                "repo": repo,
                "source": str(result_path),
                "thick": thick,
                "violations": violations,
            }
        )

    summary = format_summary(rows)
    print()
    print(summary)
    for row in rows:
        if row["violations"]:
            print(f"\n[{row['repo']}] FAIL — {row['source']}")
            for v in row["violations"]:
                print(f"  - {v}")

    if args.summary:
        Path(args.summary).write_text(
            "# lsp_boost regression check\n\n"
            + summary
            + "\n\n"
            + ("Status: FAIL\n" if any_violations else "Status: PASS\n"),
            encoding="utf-8",
        )

    return 1 if any_violations else 0


if __name__ == "__main__":
    sys.exit(main())
