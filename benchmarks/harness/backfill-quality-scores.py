#!/usr/bin/env python3
"""Backfill quality_score for existing session entries that lack one."""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import harness_eval_common as common


def main():
    entries_dir = Path.home() / ".codex" / "harness" / "reports" / "session-entries"
    if not entries_dir.exists():
        print("No session-entries directory found.")
        return

    updated = 0
    skipped = 0
    no_activity = 0

    for path in sorted(entries_dir.glob("*.json")):
        try:
            data = json.loads(path.read_text())
        except Exception:
            continue

        changed = False
        entries = []
        if isinstance(data, list):
            entries = data
        elif isinstance(data, dict) and "entries" in data:
            entries = data["entries"]
        elif isinstance(data, dict):
            entries = [data]

        for entry in entries:
            if not isinstance(entry, dict):
                continue
            if entry.get("source_kind") != "real-session":
                continue
            if entry.get("quality_score") is not None:
                skipped += 1
                continue

            score = common.compute_quality_score(entry)
            if score is None:
                no_activity += 1
                continue

            entry["quality_score"] = score
            changed = True
            updated += 1

        if changed:
            if isinstance(data, list):
                path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")
            elif isinstance(data, dict) and "entries" in data:
                data["entries"] = entries
                path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")
            else:
                path.write_text(
                    json.dumps(entries[0], ensure_ascii=False, indent=2) + "\n"
                )

    print(
        f"Updated: {updated}, Skipped (already set): {skipped}, No activity: {no_activity}"
    )


if __name__ == "__main__":
    main()
