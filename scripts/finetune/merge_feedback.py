#!/usr/bin/env python3
"""Merge verified feedback pairs into training data for next fine-tuning round.

Feedback pairs from real tool usage are higher quality than auto-generated ones.
They get 3x weight (repeated) in the merged dataset.

Usage:
  python scripts/finetune/merge_feedback.py
  # Then re-run finetune_distill.py with the merged data
"""

import json
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
AUGMENTED = SCRIPT_DIR / "training_pairs_augmented.jsonl"
FEEDBACK = SCRIPT_DIR / "feedback_pairs.jsonl"
OUTPUT = SCRIPT_DIR / "training_pairs_merged.jsonl"

FEEDBACK_WEIGHT = 3  # Repeat feedback pairs N times


def main():
    pairs = []

    # Load auto-generated pairs
    if AUGMENTED.exists():
        with open(AUGMENTED) as f:
            for line in f:
                line = line.strip()
                if line:
                    pairs.append(json.loads(line))
        print(f"Auto-generated: {len(pairs)} pairs")

    # Load verified feedback with higher weight
    feedback_count = 0
    if FEEDBACK.exists():
        with open(FEEDBACK) as f:
            for line in f:
                line = line.strip()
                if line:
                    obj = json.loads(line)
                    for _ in range(FEEDBACK_WEIGHT):
                        pairs.append(obj)
                    feedback_count += 1
        print(
            f"Feedback: {feedback_count} pairs × {FEEDBACK_WEIGHT} weight = {feedback_count * FEEDBACK_WEIGHT}"
        )

    # Write merged
    with open(OUTPUT, "w") as f:
        for p in pairs:
            f.write(json.dumps(p, ensure_ascii=False) + "\n")

    print(f"\nMerged: {len(pairs)} total → {OUTPUT}")
    print(f"  (use as --finetune-input for finetune_distill.py)")


if __name__ == "__main__":
    main()
