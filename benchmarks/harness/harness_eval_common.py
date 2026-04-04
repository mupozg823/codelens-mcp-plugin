#!/usr/bin/env python3
"""Shared helpers for harness evaluation and routing policy scripts."""

from __future__ import annotations

import json
from collections import defaultdict
from glob import glob
from pathlib import Path


def load_json(path: Path):
    return json.loads(path.read_text())


def slugify(value: str) -> str:
    chars = []
    for char in value.lower():
        if char.isalnum():
            chars.append(char)
        else:
            chars.append("-")
    slug = "".join(chars)
    while "--" in slug:
        slug = slug.replace("--", "-")
    return slug.strip("-") or "value"


def normalize_repo_id(repo_cfg):
    return repo_cfg.get("id") or Path(repo_cfg["path"]).name


def canonical_repo_key(value: str):
    return "".join(char.lower() for char in value if char.isalnum())


def infer_scenario_id(entry):
    scenario_id = entry.get("scenario_id")
    if scenario_id:
        return str(scenario_id)
    notes = entry.get("notes") or ""
    marker = "captured from scenario "
    if marker in notes:
        tail = notes.split(marker, 1)[1]
        return tail.split(" | ", 1)[0].strip() or None
    return None


def resolve_session_entry_paths(patterns):
    resolved = []
    seen = set()
    for pattern in patterns:
        for raw_path in sorted(glob(str(Path(pattern).expanduser()))):
            path = Path(raw_path).expanduser().resolve()
            if path in seen or not path.exists():
                continue
            seen.add(path)
            resolved.append(path)
    return resolved


def load_entries(paths):
    entries = []
    for path in paths:
        raw = load_json(path)
        if isinstance(raw, list):
            loaded = raw
        elif isinstance(raw, dict) and isinstance(raw.get("entries"), list):
            loaded = raw["entries"]
        else:
            loaded = [raw]
        for entry in loaded:
            if isinstance(entry, dict):
                entry.setdefault("_source_path", str(path))
                entries.append(entry)
    return entries


def canonicalize_entry_repo_ids(entries, representative_repos):
    repo_lookup = {
        str(Path(repo_cfg["path"]).expanduser().resolve()): repo_cfg
        for repo_cfg in representative_repos
    }
    repo_id_lookup = {
        canonical_repo_key(normalize_repo_id(repo_cfg)): repo_cfg
        for repo_cfg in representative_repos
    }
    for entry in entries:
        repo_cfg = repo_lookup.get(str(Path(entry.get("repo", "")).expanduser().resolve()))
        if not repo_cfg and entry.get("repo_id"):
            repo_cfg = repo_id_lookup.get(canonical_repo_key(entry["repo_id"]))
        if repo_cfg:
            entry["repo_id"] = normalize_repo_id(repo_cfg)
            entry.setdefault("repo_label", repo_cfg.get("label", normalize_repo_id(repo_cfg)))
    return entries


def load_session_entries(patterns, representative_repos):
    entries = load_entries(resolve_session_entry_paths(patterns))
    return canonicalize_entry_repo_ids(entries, representative_repos)


def dedupe_real_session_entries(entries):
    deduped = []
    latest_by_key = {}
    duplicate_buckets = defaultdict(list)

    for entry in entries:
        if entry.get("source_kind") != "real-session":
            deduped.append(entry)
            continue
        repo_id = entry.get("repo_id", entry.get("repo", ""))
        scenario_id = infer_scenario_id(entry)
        key = (
            repo_id,
            entry.get("task_kind", ""),
            entry.get("agent", ""),
            entry.get("mode", ""),
            scenario_id or "",
        )
        existing = latest_by_key.get(key)
        if existing is None or str(entry.get("_source_path", "")) > str(existing.get("_source_path", "")):
            if existing is not None:
                duplicate_buckets[key].append(existing)
            latest_by_key[key] = entry
        else:
            duplicate_buckets[key].append(entry)

    deduped.extend(latest_by_key.values())
    deduped.sort(
        key=lambda item: (
            item.get("repo_id", item.get("repo", "")),
            item.get("task_kind", ""),
            item.get("mode", ""),
            item.get("agent", ""),
            item.get("_source_path", ""),
        )
    )

    duplicates = []
    for key, duplicates_list in sorted(duplicate_buckets.items()):
        kept = latest_by_key[key]
        duplicates.append(
            {
                "repo_id": key[0],
                "task_kind": key[1],
                "agent": key[2],
                "mode": key[3],
                "scenario_id": key[4] or None,
                "kept_source_path": kept.get("_source_path"),
                "discarded_source_paths": [item.get("_source_path") for item in duplicates_list],
                "duplicate_count": len(duplicates_list),
            }
        )
    return deduped, duplicates
