#!/usr/bin/env python3
"""Archive duplicate real-session entries without deleting them."""

from __future__ import annotations

import argparse
import json
import shutil
from collections import defaultdict
from datetime import datetime
from glob import glob
from pathlib import Path

import harness_eval_common as common


DEFAULT_SESSION_GLOB = str(Path.home() / ".codex" / "harness" / "reports" / "session-entries" / "*.json")
DEFAULT_ARCHIVE_DIR = Path.home() / ".codex" / "harness" / "reports" / "session-entries" / "archive" / "duplicates"


def resolve_paths(patterns):
    resolved = []
    seen = set()
    for pattern in patterns:
        for raw_path in sorted(glob(str(Path(pattern).expanduser()))):
            path = Path(raw_path).expanduser().resolve()
            if path in seen or not path.is_file():
                continue
            seen.add(path)
            resolved.append(path)
    return resolved


def logical_key(entry):
    repo_id = entry.get("repo_id") or Path(entry.get("repo", "")).name
    return (
        repo_id,
        entry.get("task_kind", ""),
        (entry.get("agent") or "").strip().lower(),
        entry.get("mode", ""),
        common.infer_scenario_id(entry) or "",
    )


def collect_duplicates(paths):
    latest_by_key = {}
    duplicate_buckets = defaultdict(list)

    for path in paths:
        entry = common.load_json(path)
        if not isinstance(entry, dict) or entry.get("source_kind") != "real-session":
            continue
        key = logical_key(entry)
        existing = latest_by_key.get(key)
        candidate = {"path": path, "entry": entry}
        if existing is None or path.name > existing["path"].name:
            if existing is not None:
                duplicate_buckets[key].append(existing)
            latest_by_key[key] = candidate
        else:
            duplicate_buckets[key].append(candidate)

    duplicates = []
    for key, discarded in sorted(duplicate_buckets.items()):
        kept = latest_by_key[key]
        duplicates.append(
            {
                "repo_id": key[0],
                "task_kind": key[1],
                "agent": key[2],
                "mode": key[3],
                "scenario_id": key[4] or None,
                "kept": str(kept["path"]),
                "discarded": [str(item["path"]) for item in discarded],
                "duplicate_count": len(discarded),
            }
        )
    return duplicates


def render_markdown(payload):
    lines = [
        "# Session Entry Duplicate Archive",
        "",
        f"- Generated: `{payload['generated_at']}`",
        f"- Apply mode: `{payload['applied']}`",
        f"- Duplicate buckets: `{payload['duplicate_bucket_count']}`",
        f"- Duplicate files: `{payload['duplicate_file_count']}`",
        "",
    ]
    if not payload["duplicates"]:
        lines.append("No duplicate real-session entries found.")
        return "\n".join(lines) + "\n"

    for row in payload["duplicates"]:
        lines.append(f"- `{row['repo_id']} / {row['task_kind']} / {row['agent']} / {row['mode']}`")
        lines.append(f"  - kept: `{row['kept']}`")
        lines.append(f"  - discarded: `{row['duplicate_count']}`")
        if row.get("archived"):
            lines.append(f"  - archived_to: `{row['archived']}`")
    lines.append("")
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--session-entry-glob", action="append", default=[])
    parser.add_argument("--archive-dir", default=str(DEFAULT_ARCHIVE_DIR))
    parser.add_argument("--output-json")
    parser.add_argument("--output-md")
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()

    patterns = args.session_entry_glob or [DEFAULT_SESSION_GLOB]
    paths = resolve_paths(patterns)
    duplicates = collect_duplicates(paths)
    archive_dir = Path(args.archive_dir).expanduser()

    moved_files = []
    if args.apply and duplicates:
        archive_dir.mkdir(parents=True, exist_ok=True)
        for row in duplicates:
            archived = []
            for discarded_path in row["discarded"]:
                src = Path(discarded_path)
                dst = archive_dir / src.name
                counter = 1
                while dst.exists():
                    dst = archive_dir / f"{src.stem}-{counter}{src.suffix}"
                    counter += 1
                shutil.move(str(src), str(dst))
                archived.append(str(dst))
                moved_files.append(str(dst))
            row["archived"] = archived

    payload = {
        "generated_at": datetime.now().isoformat(timespec="seconds"),
        "applied": args.apply,
        "archive_dir": str(archive_dir),
        "resolved_session_entries": [str(path) for path in paths],
        "duplicate_bucket_count": len(duplicates),
        "duplicate_file_count": sum(len(row["discarded"]) for row in duplicates),
        "duplicates": duplicates,
        "moved_files": moved_files,
    }

    if args.output_json:
        Path(args.output_json).expanduser().write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n")
    if args.output_md:
        Path(args.output_md).expanduser().write_text(render_markdown(payload))
    print(json.dumps(payload, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
