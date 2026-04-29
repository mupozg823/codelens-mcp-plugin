#!/usr/bin/env python3
"""Compare Criterion benchmark results against a saved baseline.

Usage:
    # Save baseline (run after a known-good commit)
    cargo bench -p codelens-engine --bench search_paths -- --save-baseline main
    python3 scripts/bench-compare.py --save-baseline main

    # Compare current results against baseline
    cargo bench -p codelens-engine --bench search_paths -- --baseline main
    python3 scripts/bench-compare.py --baseline main --threshold 10

    # JSON output for CI
    python3 scripts/bench-compare.py --baseline main --json

Exit code:
    0 → no regression
    1 → regression detected (or error)
"""

import argparse
import json
import os
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

BENCH_DIR = Path("target/criterion")
BASELINES_DIR = Path(".bench-baselines")


@dataclass
class BenchResult:
    name: str
    mean_ns: float
    stddev_ns: float


def parse_criterion_json(report_dir: Path) -> list[BenchResult]:
    results = []
    for est_file in report_dir.rglob("estimates.json"):
        try:
            data = json.loads(est_file.read_text())
            mean = data.get("mean", {})
            point = mean.get("point_estimate", 0.0)
            stddev = mean.get("standard_error", 0.0)
            # Extract benchmark name from path
            rel = est_file.relative_to(report_dir)
            name = str(rel.parent).replace("/", "::")
            results.append(BenchResult(name=name, mean_ns=point, stddev_ns=stddev))
        except (json.JSONDecodeError, KeyError):
            continue
    return results


def save_baseline(name: str) -> None:
    BASELINES_DIR.mkdir(exist_ok=True)
    baseline_file = BASELINES_DIR / f"{name}.json"
    results = parse_criterion_json(BENCH_DIR)
    if not results:
        print("No benchmark results found in target/criterion", file=sys.stderr)
        sys.exit(1)
    data = [{"name": r.name, "mean_ns": r.mean_ns, "stddev_ns": r.stddev_ns} for r in results]
    baseline_file.write_text(json.dumps(data, indent=2))
    print(f"Saved {len(results)} benchmarks to {baseline_file}")


def compare_baseline(name: str, threshold_pct: float, json_out: bool) -> bool:
    baseline_file = BASELINES_DIR / f"{name}.json"
    if not baseline_file.exists():
        print(f"Baseline '{name}' not found. Run with --save-baseline first.", file=sys.stderr)
        sys.exit(1)

    baseline = {item["name"]: item for item in json.loads(baseline_file.read_text())}
    current = parse_criterion_json(BENCH_DIR)

    regressions = []
    improvements = []
    missing = []
    ok = []

    for cur in current:
        base = baseline.get(cur.name)
        if not base:
            missing.append(cur.name)
            continue
        base_mean = base["mean_ns"]
        change_pct = ((cur.mean_ns - base_mean) / base_mean) * 100 if base_mean else 0

        if change_pct > threshold_pct:
            regressions.append((cur.name, base_mean, cur.mean_ns, change_pct))
        elif change_pct < -threshold_pct:
            improvements.append((cur.name, base_mean, cur.mean_ns, change_pct))
        else:
            ok.append((cur.name, change_pct))

    for name in baseline:
        if not any(c.name == name for c in current):
            missing.append(name)

    if json_out:
        output = {
            "regressions": [
                {"name": n, "base_ms": b / 1e6, "current_ms": c / 1e6, "change_pct": p}
                for n, b, c, p in regressions
            ],
            "improvements": [
                {"name": n, "base_ms": b / 1e6, "current_ms": c / 1e6, "change_pct": p}
                for n, b, c, p in improvements
            ],
            "missing": missing,
            "ok_count": len(ok),
            "regression_detected": len(regressions) > 0,
        }
        print(json.dumps(output, indent=2))
    else:
        print(f"Threshold: {threshold_pct}%")
        print(f"Benchmarks: {len(current)} current vs {len(baseline)} baseline")
        print()
        if regressions:
            print("❌ REGRESSIONS:")
            for name, base, cur, pct in regressions:
                print(f"  {name}: {base/1e6:.3f}ms → {cur/1e6:.3f}ms (+{pct:.1f}%)")
            print()
        if improvements:
            print("📉 IMPROVEMENTS:")
            for name, base, cur, pct in improvements:
                print(f"  {name}: {base/1e6:.3f}ms → {cur/1e6:.3f}ms ({pct:.1f}%)")
            print()
        if missing:
            print(f"⚠️  MISSING: {', '.join(missing)}")
            print()
        if not regressions and not improvements:
            print("✅ All benchmarks within threshold.")

    return len(regressions) == 0


def main():
    parser = argparse.ArgumentParser(description="Benchmark baseline manager")
    parser.add_argument("--save-baseline", help="Save current results as baseline")
    parser.add_argument("--baseline", help="Compare against this baseline")
    parser.add_argument("--threshold", type=float, default=10.0, help="Regression threshold %% (default: 10)")
    parser.add_argument("--json", action="store_true", help="Output JSON for CI")
    args = parser.parse_args()

    if args.save_baseline:
        save_baseline(args.save_baseline)
    elif args.baseline:
        ok = compare_baseline(args.baseline, args.threshold, args.json)
        sys.exit(0 if ok else 1)
    else:
        parser.print_help()
        sys.exit(1)


if __name__ == "__main__":
    main()
