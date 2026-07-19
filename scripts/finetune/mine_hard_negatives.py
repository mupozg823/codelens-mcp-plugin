#!/usr/bin/env python3
"""Mine dense hard negatives for CodeLens LoRA training.

Thin wrapper around sentence-transformers `mine_hard_negatives`. The heavy ML
dependencies (torch, sentence-transformers, datasets) are imported lazily inside
the mining path so `--dry-run` stays CI-safe: it validates configuration and
emits a mining-plan JSON without importing any ML library.

The output JSONL is consumed by `train_codelens_lora.py --hard-negatives`.

Usage:
    # CI-safe contract check (no ML deps imported):
    python3 scripts/finetune/mine_hard_negatives.py --dry-run

    # Real mining in an ML environment:
    python3 scripts/finetune/mine_hard_negatives.py \
        --train-data scripts/finetune/curated_1k_pairs.jsonl \
        --output scripts/finetune/hard_negatives.jsonl \
        --num-negatives 5
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

SCRIPT_DIR = Path(__file__).resolve().parent
DEFAULT_TRAIN_DATA = SCRIPT_DIR / "curated_1k_pairs.jsonl"
DEFAULT_OUTPUT = SCRIPT_DIR / "hard_negatives.jsonl"
DEFAULT_MODEL = "sentence-transformers/all-MiniLM-L12-v2"

PLAN_SCHEMA_VERSION = "codelens-hard-negative-mining-plan-v1"


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Mine dense hard negatives via sentence-transformers."
    )
    parser.add_argument("--train-data", default=str(DEFAULT_TRAIN_DATA))
    parser.add_argument("--output", default=str(DEFAULT_OUTPUT))
    parser.add_argument("--model", default=DEFAULT_MODEL)
    parser.add_argument("--query-field", default="query")
    parser.add_argument("--positive-field", default="positive")
    parser.add_argument(
        "--num-negatives",
        type=int,
        default=5,
        help="Negatives mined per (query, positive) pair.",
    )
    parser.add_argument(
        "--sampling-strategy",
        default="top",
        choices=["top", "random"],
        help="'top' keeps the hardest candidates; 'random' samples within range.",
    )
    parser.add_argument(
        "--relative-margin",
        type=float,
        default=0.05,
        help=(
            "False-negative guard (minor parameter): reject a candidate whose "
            "similarity is within this relative margin of the positive, so true "
            "positives are not mislabeled as negatives."
        ),
    )
    parser.add_argument(
        "--margin",
        type=float,
        default=None,
        help="Optional absolute similarity margin guard (None disables it).",
    )
    parser.add_argument(
        "--range-min",
        type=int,
        default=0,
        help="Skip the top-N most similar candidates (likely positives).",
    )
    parser.add_argument(
        "--range-max",
        type=int,
        default=None,
        help="Only consider candidates up to this rank (None = no cap).",
    )
    parser.add_argument("--batch-size", type=int, default=32)
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Write the mining plan JSON without importing ML deps.",
    )
    return parser.parse_args(argv)


def build_plan(args: argparse.Namespace) -> dict[str, Any]:
    return {
        "schema_version": PLAN_SCHEMA_VERSION,
        "train_data": str(args.train_data),
        "output": str(args.output),
        "model": args.model,
        "query_field": args.query_field,
        "positive_field": args.positive_field,
        "params": {
            "num_negatives": args.num_negatives,
            "sampling_strategy": args.sampling_strategy,
            # relative_margin / margin are the false-negative guards.
            "relative_margin": args.relative_margin,
            "margin": args.margin,
            "range_min": args.range_min,
            "range_max": args.range_max,
            "batch_size": args.batch_size,
        },
        "output_schema": {
            "query": "str",
            "positive": "str",
            "negative": "str",
        },
    }


def plan_path_for(output: str | Path) -> Path:
    return Path(output).with_suffix(".mining-plan.json")


def load_pairs(path: Path, query_field: str, positive_field: str) -> list[dict[str, str]]:
    if not path.exists():
        raise SystemExit(f"Training data not found: {path}")
    rows: list[dict[str, str]] = []
    with path.open(encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            obj = json.loads(line)
            query = str(obj.get(query_field, "")).strip()
            positive = str(obj.get(positive_field, "")).strip()
            if query and positive:
                rows.append({"query": query, "positive": positive})
    return rows


def mine(args: argparse.Namespace) -> Path:
    """Run the real mining pass (ML deps imported lazily here)."""
    try:
        from datasets import Dataset
        from sentence_transformers import SentenceTransformer
        from sentence_transformers.util import mine_hard_negatives
    except ImportError as exc:
        raise SystemExit(
            "Missing mining dependency. Install sentence-transformers and "
            "datasets before running without --dry-run."
        ) from exc

    rows = load_pairs(
        Path(args.train_data), args.query_field, args.positive_field
    )
    if not rows:
        raise SystemExit("No valid (query, positive) pairs found")

    dataset = Dataset.from_dict(
        {
            "anchor": [row["query"] for row in rows],
            "positive": [row["positive"] for row in rows],
        }
    )
    model = SentenceTransformer(args.model)
    mined = mine_hard_negatives(
        dataset,
        model,
        num_negatives=args.num_negatives,
        sampling_strategy=args.sampling_strategy,
        relative_margin=args.relative_margin,
        margin=args.margin,
        range_min=args.range_min,
        range_max=args.range_max,
        batch_size=args.batch_size,
        output_format="triplet",
    )

    output_path = Path(args.output)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    with output_path.open("w", encoding="utf-8") as handle:
        for record in mined:
            handle.write(
                json.dumps(
                    {
                        "query": record["anchor"],
                        "positive": record["positive"],
                        "negative": record["negative"],
                    },
                    ensure_ascii=False,
                )
                + "\n"
            )
    print(f"Wrote hard negatives: {output_path} ({len(mined)} triplets)")
    return output_path


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    plan = build_plan(args)

    plan_path = plan_path_for(args.output)
    plan_path.parent.mkdir(parents=True, exist_ok=True)
    plan_path.write_text(
        json.dumps(plan, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(plan, indent=2, sort_keys=True))
    print(f"Wrote mining plan: {plan_path}")

    if args.dry_run:
        return 0

    mine(args)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
