#!/usr/bin/env python3
"""Aggregate external embedding-quality reports into a phase matrix.

This exists to stop hand-maintained phase tables from drifting as more
external datasets are added. It scans the versioned phase3 report files,
groups the four benchmark arms for each dataset, and emits a compact JSON
and markdown summary that can be referenced from docs or review notes.
"""

from __future__ import annotations

import argparse
import glob
import json
import math
import re
from pathlib import Path


ARM_ORDER = ("baseline", "2e-only", "2b2c-only", "stacked")
HYBRID_METHOD = "get_ranked_context"
DEFAULT_GLOB = "benchmarks/embedding-quality-v1.*-phase3*-*.json"

DATASET_REGISTRY = {
    "ripgrep": {"label": "ripgrep external", "language": "Rust", "archetype": "tooling"},
    "requests": {
        "label": "requests external",
        "language": "Python",
        "archetype": "app library",
    },
    "jest": {"label": "jest external", "language": "TS/JS", "archetype": "tooling"},
    "typescript": {
        "label": "typescript external",
        "language": "TS/JS",
        "archetype": "compiler",
    },
    "next-js": {
        "label": "next-js external",
        "language": "TS/JS",
        "archetype": "typical app",
    },
    "react-core": {
        "label": "react-core external",
        "language": "TS/JS",
        "archetype": "short runtime",
    },
    "django": {
        "label": "django external",
        "language": "Python",
        "archetype": "framework",
    },
}

FILENAME_RE = re.compile(
    r"embedding-quality-(?P<version>v\d+\.\d+)-(?P<phase>phase3[a-z])-(?P<slug>.+)-(?P<arm>baseline|2e-only|2b2c-only|stacked)\.json$"
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--glob", default=DEFAULT_GLOB)
    parser.add_argument(
        "--output-json",
        default="benchmarks/embedding-quality-phase3-matrix.json",
    )
    parser.add_argument(
        "--output-md",
        default="benchmarks/embedding-quality-phase3-matrix.md",
    )
    parser.add_argument(
        "--flat-relative-threshold-pct",
        type=float,
        default=1.0,
        help="Relative delta band treated as flat/inert.",
    )
    parser.add_argument(
        "--strong-relative-threshold-pct",
        type=float,
        default=5.0,
        help="Relative delta band treated as strong positive/negative.",
    )
    parser.add_argument(
        "--require-datasets",
        default="",
        help="Comma-separated dataset slugs that must appear in the aggregated matrix.",
    )
    return parser.parse_args()


def load_report(path: Path) -> dict:
    data = json.loads(path.read_text(encoding="utf-8"))
    methods = {method["method"]: method for method in data.get("methods", [])}
    hybrid = methods.get(HYBRID_METHOD)
    if hybrid is None:
        raise SystemExit(f"{path}: missing {HYBRID_METHOD}")
    return {
        "path": str(path),
        "raw": data,
        "methods": methods,
        "hybrid": hybrid,
    }


def dataset_slug_from_path(dataset_path: str) -> str:
    name = Path(dataset_path).name
    prefix = "embedding-quality-dataset-"
    suffix = ".json"
    if name.startswith(prefix) and name.endswith(suffix):
        return name[len(prefix) : -len(suffix)]
    return Path(dataset_path).stem


def effect_band(delta_rel_pct: float | None, flat_threshold_pct: float, strong_threshold_pct: float) -> str:
    if delta_rel_pct is None or math.isnan(delta_rel_pct):
        return "undefined"
    if abs(delta_rel_pct) < flat_threshold_pct:
        return "flat"
    if delta_rel_pct >= strong_threshold_pct:
        return "strong positive"
    if delta_rel_pct > 0:
        return "mild positive"
    if delta_rel_pct <= -strong_threshold_pct:
        return "strong negative"
    return "mild negative"


def build_matrix(
    paths: list[Path],
    flat_threshold_pct: float,
    strong_threshold_pct: float,
    report_glob: str,
) -> dict:
    groups: dict[tuple[str, str, str], dict[str, dict]] = {}
    for path in paths:
        match = FILENAME_RE.match(path.name)
        if not match:
            continue
        info = match.groupdict()
        report = load_report(path)
        slug = dataset_slug_from_path(report["raw"]["dataset_path"])
        key = (info["version"], info["phase"], slug)
        groups.setdefault(key, {})
        groups[key][info["arm"]] = report

    rows = []
    for (version, phase, slug), arms in sorted(groups.items(), key=lambda item: item[0][1]):
        missing = [arm for arm in ARM_ORDER if arm not in arms]
        if missing:
            raise SystemExit(f"{version}/{phase}/{slug}: missing arms {missing}")

        first = arms["baseline"]["raw"]
        dataset_size = first["dataset_size"]
        dataset_path = first["dataset_path"]
        for arm in ARM_ORDER[1:]:
            report = arms[arm]["raw"]
            if report["dataset_size"] != dataset_size:
                raise SystemExit(f"{version}/{phase}/{slug}: dataset_size mismatch")
            if report["dataset_path"] != dataset_path:
                raise SystemExit(f"{version}/{phase}/{slug}: dataset_path mismatch")

        baseline = arms["baseline"]["hybrid"]["mrr"]
        stacked = arms["stacked"]["hybrid"]["mrr"]
        delta_abs = stacked - baseline
        delta_rel_pct = None if baseline == 0 else (delta_abs / baseline) * 100.0
        registry = DATASET_REGISTRY.get(slug, {})

        rows.append(
            {
                "version": version,
                "phase": phase,
                "dataset_slug": slug,
                "label": registry.get("label", slug),
                "language": registry.get("language", "unknown"),
                "archetype": registry.get("archetype", "unknown"),
                "dataset_path": dataset_path,
                "dataset_size": dataset_size,
                "runtime_model": first.get("runtime_model"),
                "arms": {
                    arm: {
                        "path": arms[arm]["path"],
                        "hybrid_mrr": arms[arm]["hybrid"]["mrr"],
                        "hybrid_acc1": arms[arm]["hybrid"]["acc1"],
                        "hybrid_acc3": arms[arm]["hybrid"]["acc3"],
                        "semantic_mrr": arms[arm]["methods"]["semantic_search"]["mrr"],
                        "lexical_mrr": arms[arm]["methods"]["get_ranked_context_no_semantic"]["mrr"],
                    }
                    for arm in ARM_ORDER
                },
                "delta_abs": delta_abs,
                "delta_rel_pct": delta_rel_pct,
                "effect_band": effect_band(
                    delta_rel_pct,
                    flat_threshold_pct=flat_threshold_pct,
                    strong_threshold_pct=strong_threshold_pct,
                ),
            }
        )

    counts = {"positive": 0, "negative": 0, "flat": 0}
    for row in rows:
        if row["effect_band"].endswith("positive"):
            counts["positive"] += 1
        elif row["effect_band"].endswith("negative"):
            counts["negative"] += 1
        else:
            counts["flat"] += 1

    return {
        "report_glob": report_glob,
        "source_report_count": len(paths),
        "flat_relative_threshold_pct": flat_threshold_pct,
        "strong_relative_threshold_pct": strong_threshold_pct,
        "dataset_count": len(rows),
        "effect_counts": counts,
        "rows": rows,
    }


def fmt(value: float | None, digits: int = 3) -> str:
    if value is None:
        return "—"
    return f"{value:.{digits}f}"


def fmt_rel_pct(value: float | None) -> str:
    if value is None:
        return "—"
    sign = "+" if value >= 0 else ""
    return f"{sign}{value:.1f}%"


def render_markdown(summary: dict) -> str:
    lines: list[str] = []
    a = lines.append

    a("# External Embedding Quality Matrix")
    a("")
    a(
        f"- Datasets: {summary['dataset_count']} (`positive={summary['effect_counts']['positive']}`, "
        f"`negative={summary['effect_counts']['negative']}`, `flat={summary['effect_counts']['flat']}`)"
    )
    a(
        f"- Flat band: relative delta under `{summary['flat_relative_threshold_pct']:.1f}%`"
    )
    a(
        f"- Strong band: relative delta at or above `{summary['strong_relative_threshold_pct']:.1f}%`"
    )
    a("")
    a("| Phase | Dataset | Language / archetype | Baseline | 2e | 2b+2c | Stacked | Δ abs | Δ rel | Band |")
    a("|---|---|---|---:|---:|---:|---:|---:|---:|---|")
    for row in summary["rows"]:
        arms = row["arms"]
        a(
            f"| {row['phase']} | {row['label']} | {row['language']} / {row['archetype']} | "
            f"{fmt(arms['baseline']['hybrid_mrr'])} | "
            f"{fmt(arms['2e-only']['hybrid_mrr'])} | "
            f"{fmt(arms['2b2c-only']['hybrid_mrr'])} | "
            f"{fmt(arms['stacked']['hybrid_mrr'])} | "
            f"{fmt(row['delta_abs'])} | "
            f"{fmt_rel_pct(row['delta_rel_pct'])} | "
            f"{row['effect_band']} |"
        )

    a("")
    a("## Artefacts")
    a("")
    for row in summary["rows"]:
        a(f"- `{row['phase']}` / `{row['label']}`")
        for arm in ARM_ORDER:
            a(f"  - `{arm}`: `{row['arms'][arm]['path']}`")

    a("")
    return "\n".join(lines)


def main() -> None:
    args = parse_args()
    paths = [Path(path) for path in sorted(glob.glob(args.glob))]
    if not paths:
        raise SystemExit(f"no files matched: {args.glob}")

    summary = build_matrix(
        paths,
        flat_threshold_pct=args.flat_relative_threshold_pct,
        strong_threshold_pct=args.strong_relative_threshold_pct,
        report_glob=args.glob,
    )

    if args.require_datasets.strip():
        expected = {
            item.strip()
            for item in args.require_datasets.split(",")
            if item.strip()
        }
        actual = {row["dataset_slug"] for row in summary["rows"]}
        missing = sorted(expected - actual)
        unexpected = sorted(actual - expected)
        if missing or unexpected:
            problems = []
            if missing:
                problems.append(f"missing={missing}")
            if unexpected:
                problems.append(f"unexpected={unexpected}")
            raise SystemExit("dataset set mismatch: " + ", ".join(problems))

    output_json = Path(args.output_json)
    output_md = Path(args.output_md)
    output_json.write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    output_md.write_text(render_markdown(summary) + "\n", encoding="utf-8")
    print(json.dumps(summary, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
