#!/usr/bin/env python3
"""Shared project discovery for benchmark scripts."""

from __future__ import annotations

import glob
import os


EXCLUDE_DIRS = {
    "node_modules",
    ".venv",
    "venv",
    "target",
    "dist",
    "build",
    "__pycache__",
    ".git",
}

SOURCE_EXTENSIONS = [".rs", ".py", ".ts", ".js", ".go", ".java"]

CANDIDATE_SYMBOLS = [
    "dispatch_tool",
    "handle_request",
    "process",
    "execute",
    "run_server",
    "parse_args",
    "build",
    "create_app",
    "init",
    "setup",
]


def _filtered_source_files(project: str, extension: str):
    all_files = glob.glob(os.path.join(project, "**/*" + extension), recursive=True)
    return [
        path for path in all_files if not any(part in path.split(os.sep) for part in EXCLUDE_DIRS)
    ]


def discover_project_context(project: str, codelens):
    info_out, _, _, info_payload = codelens("get_project_structure", {}, preset="balanced")
    total_files = total_symbols = "?"
    if info_payload:
        try:
            total_files = info_payload["data"]["total_files"]
            total_symbols = info_payload["data"]["total_symbols"]
        except Exception:
            pass

    ext_counts = {}
    for ext in SOURCE_EXTENSIONS:
        filtered = _filtered_source_files(project, ext)
        if filtered:
            ext_counts[ext] = len(filtered)
    primary_ext = max(ext_counts, key=ext_counts.get) if ext_counts else ".rs"
    grep_include = "*" + primary_ext

    test_symbol = None
    test_file = None
    for candidate in CANDIDATE_SYMBOLS:
        _, _, _, sym_payload = codelens(
            "find_symbol", {"name": candidate, "max_matches": 1}, preset="balanced"
        )
        if not sym_payload:
            continue
        try:
            if sym_payload.get("data", {}).get("count", 0) > 0:
                symbol = sym_payload["data"]["symbols"][0]
                test_symbol = symbol.get("name", candidate)
                test_file = symbol.get("file_path")
                break
        except Exception:
            continue

    if not test_symbol:
        _, _, _, sym_payload = codelens(
            "find_symbol", {"name": "main", "max_matches": 1}, preset="balanced"
        )
        test_symbol = "main"
        if sym_payload:
            try:
                if sym_payload.get("data", {}).get("count", 0) > 0:
                    symbol = sym_payload["data"]["symbols"][0]
                    test_symbol = symbol.get("name", "main")
                    test_file = symbol.get("file_path")
            except Exception:
                pass

    _, _, _, onboard_payload = codelens(
        "onboard_project", {}, timeout=30, preset="balanced"
    )
    key_file = None
    key_files_list = []
    if onboard_payload:
        try:
            key_files = onboard_payload.get("data", {}).get("key_files", [])
            key_files_list = [item["file"] for item in key_files[:5]]
            if key_files:
                key_file = key_files[0]["file"]
        except Exception:
            pass

    if not test_file:
        all_src = glob.glob(os.path.join(project, "**/*" + primary_ext), recursive=True)
        if all_src:
            test_file = os.path.relpath(all_src[0], project)
    if not key_file:
        key_file = test_file

    return {
        "total_files": total_files,
        "total_symbols": total_symbols,
        "primary_ext": primary_ext,
        "grep_include": grep_include,
        "test_symbol": test_symbol,
        "test_file": test_file,
        "key_file": key_file,
        "key_files_list": key_files_list,
        "ext_counts": ext_counts,
    }
