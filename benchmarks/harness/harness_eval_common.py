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


def build_repo_lookups(representative_repos):
    repo_lookup = {
        str(Path(repo_cfg["path"]).expanduser().resolve()): repo_cfg
        for repo_cfg in representative_repos
    }
    repo_id_lookup = {
        canonical_repo_key(normalize_repo_id(repo_cfg)): repo_cfg
        for repo_cfg in representative_repos
    }
    return repo_lookup, repo_id_lookup


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
    repo_lookup, repo_id_lookup = build_repo_lookups(representative_repos)
    for entry in entries:
        repo_cfg = repo_lookup.get(
            str(Path(entry.get("repo", "")).expanduser().resolve())
        )
        if not repo_cfg and entry.get("repo_id"):
            repo_cfg = repo_id_lookup.get(canonical_repo_key(entry["repo_id"]))
        if repo_cfg:
            entry["repo_id"] = normalize_repo_id(repo_cfg)
            entry["repo_label"] = repo_cfg.get("label", normalize_repo_id(repo_cfg))
    return entries


def load_session_entries(patterns, representative_repos):
    entries = load_entries(resolve_session_entry_paths(patterns))
    return canonicalize_entry_repo_ids(entries, representative_repos)


def qualifying_real_entry(entry):
    if entry.get("source_kind") != "real-session":
        return False
    if entry.get("success") is not True:
        return False
    if entry.get("acceptance_passed") is False:
        return False
    if entry.get("verify_passed") is False:
        return False
    return True


def real_session_identity(entry):
    return (
        entry.get("repo_id", entry.get("repo", "")),
        entry.get("task_kind", ""),
        (entry.get("agent") or "").strip().lower(),
        entry.get("mode", ""),
        infer_scenario_id(entry) or "",
    )


def dedupe_real_session_entries(entries, include_entry=None):
    deduped = []
    latest_by_key = {}
    duplicate_buckets = defaultdict(list)

    for entry in entries:
        if entry.get("source_kind") != "real-session":
            deduped.append(entry)
            continue
        if include_entry is not None and not include_entry(entry):
            continue
        key = real_session_identity(entry)
        existing = latest_by_key.get(key)
        if existing is None or str(entry.get("_source_path", "")) > str(
            existing.get("_source_path", "")
        ):
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
                "discarded_source_paths": [
                    item.get("_source_path") for item in duplicates_list
                ],
                "duplicate_count": len(duplicates_list),
            }
        )
    return deduped, duplicates


def compute_quality_score(entry):
    """Compute quality_score from session metrics when not manually set.

    Returns float 0.0-1.0 or None if insufficient data.
    Weights:
      - error_free (0.3): no errors during session
      - verifier_used (0.2): verifier contract was present
      - verifier_followthrough (0.2): verifier checks were actually followed
      - evidence_reuse (0.15): analysis handles were reused
      - composite_usage (0.15): workflow tools were used (not just primitives)
    """
    metrics = entry.get("metrics_snapshot") or {}
    error_count = int(metrics.get("error_count") or 0)
    tool_calls = int(entry.get("tool_calls") or 0)

    # Skip entries with no real tool activity
    if tool_calls == 0:
        return None

    error_free = 1.0 if error_count == 0 else 0.0
    verifier = 1.0 if entry.get("verifier_used") else 0.0
    vf_rate = float(metrics.get("verifier_followthrough_rate") or 0.0)
    evidence = min(float(entry.get("evidence_reuse_rate") or 0.0), 1.0)
    composite = min(float(entry.get("composite_ratio") or 0.0), 1.0)

    score = (
        0.30 * error_free
        + 0.20 * verifier
        + 0.20 * vf_rate
        + 0.15 * evidence
        + 0.15 * composite
    )
    return round(score, 3)


def filter_qualifying_entries(entries):
    """Return only entries that are synthetic OR qualifying real-sessions."""
    return [
        e
        for e in entries
        if e.get("source_kind") != "real-session" or qualifying_real_entry(e)
    ]


def compare_policy_structure(policy_a, policy_b):
    """Compare two policies structurally. Returns {identical: bool, differences: list}."""
    struct_a = policy_structure(policy_a)
    struct_b = policy_structure(policy_b)
    differences = []
    for key in sorted(set(struct_a) | set(struct_b)):
        if struct_a.get(key) != struct_b.get(key):
            differences.append(
                {"field": key, "a": struct_a.get(key), "b": struct_b.get(key)}
            )
    return {"identical": not differences, "differences": differences}


def policy_structure(policy):
    return {
        "schema_version": policy.get("schema_version"),
        "policy_scope": policy.get("policy_scope"),
        "agent": policy.get("agent"),
        "source_of_truth": policy.get("source_of_truth"),
        "runtime_authority": policy.get("runtime_authority"),
        "source_report": policy.get("source_report"),
        "source_report_path": policy.get("source_report_path"),
        "binary": policy.get("binary"),
        "global_rules": sorted(
            [
                {
                    "task_kind": row.get("task_kind"),
                    "recommended_policy": row.get("recommended_policy"),
                    "consensus": row.get("consensus"),
                    "repo_count": row.get("repo_count"),
                    "vote_count": row.get("vote_count"),
                    "explanation": row.get("explanation"),
                }
                for row in policy.get("global_rules", [])
            ],
            key=lambda row: (
                row.get("task_kind") or "",
                row.get("recommended_policy") or "",
            ),
        ),
        "repo_overrides": sorted(
            [
                {
                    "repo_id": row.get("repo_id"),
                    "repo_label": row.get("repo_label"),
                    "task_kind": row.get("task_kind"),
                    "recommended_policy": row.get("recommended_policy"),
                    "confidence": row.get("confidence"),
                    "explanation": row.get("explanation"),
                }
                for row in policy.get("repo_overrides", [])
            ],
            key=lambda row: (
                row.get("repo_id") or "",
                row.get("task_kind") or "",
                row.get("recommended_policy") or "",
            ),
        ),
    }
