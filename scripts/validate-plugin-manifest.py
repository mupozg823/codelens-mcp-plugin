#!/usr/bin/env python3
"""Validate the .claude-plugin manifests (plugin.json + marketplace.json).

Deterministic structure gate, mirroring scripts/surface-manifest.py --check.
Verifies JSON validity, required fields, that bundled skills/agents directories
exist and are non-empty, and that the marketplace entry is consistent with
plugin.json. The codelens-mcp binary is an out-of-band prerequisite and is NOT
checked here.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]

PLUGIN_MANIFEST = ".claude-plugin/plugin.json"
MARKETPLACE_MANIFEST = ".claude-plugin/marketplace.json"
REQUIRED_PLUGIN_FIELDS = ("name", "version", "description", "mcpServers")
REQUIRED_MARKET_FIELDS = ("name", "owner", "plugins")


def _load_json(path: Path, label: str):
    if not path.exists():
        return None, f"{label}: missing"
    try:
        return json.loads(path.read_text(encoding="utf-8")), None
    except json.JSONDecodeError as exc:
        return None, f"{label}: invalid JSON ({exc})"


def collect_manifest_errors(repo_root: Path) -> list[str]:
    errors: list[str] = []

    plugin, err = _load_json(repo_root / PLUGIN_MANIFEST, PLUGIN_MANIFEST)
    if err:
        errors.append(err)
    if isinstance(plugin, dict):
        for field in REQUIRED_PLUGIN_FIELDS:
            if field not in plugin:
                errors.append(f"{PLUGIN_MANIFEST}: missing required field '{field}'")

        servers = plugin.get("mcpServers")
        if servers is not None and not isinstance(servers, dict):
            errors.append(f"{PLUGIN_MANIFEST}: mcpServers must be an object")
        elif isinstance(servers, dict):
            entry = servers.get("codelens")
            if (
                not isinstance(entry, dict)
                or not isinstance(entry.get("command"), str)
                or not entry.get("command")
            ):
                errors.append(
                    f"{PLUGIN_MANIFEST}: mcpServers.codelens.command must be a non-empty string"
                )

        for key in ("skills", "agents"):
            rel = plugin.get(key)
            if rel is None:
                continue
            directory = (repo_root / rel).resolve()
            if not directory.is_dir() or not any(directory.iterdir()):
                errors.append(
                    f"{PLUGIN_MANIFEST}: {key} path '{rel}' is not a non-empty directory"
                )

    market, err = _load_json(repo_root / MARKETPLACE_MANIFEST, MARKETPLACE_MANIFEST)
    if err:
        errors.append(err)
    if isinstance(market, dict):
        for field in REQUIRED_MARKET_FIELDS:
            if field not in market:
                errors.append(f"{MARKETPLACE_MANIFEST}: missing required field '{field}'")

        plugins = market.get("plugins")
        if not isinstance(plugins, list) or not plugins:
            errors.append(f"{MARKETPLACE_MANIFEST}: 'plugins' must be a non-empty array")
        else:
            plugin_name = plugin.get("name") if isinstance(plugin, dict) else None
            for i, item in enumerate(plugins):
                if not isinstance(item, dict):
                    errors.append(f"{MARKETPLACE_MANIFEST}: plugins[{i}] must be an object")
                    continue
                if not item.get("source"):
                    errors.append(f"{MARKETPLACE_MANIFEST}: plugins[{i}].source missing")
                if not item.get("name"):
                    errors.append(f"{MARKETPLACE_MANIFEST}: plugins[{i}].name missing")
                elif (
                    item.get("source") == "./"
                    and plugin_name is not None
                    and item["name"] != plugin_name
                ):
                    errors.append(
                        f"{MARKETPLACE_MANIFEST}: plugins[{i}].name '{item['name']}' "
                        f"does not match plugin.json name '{plugin_name}'"
                    )

    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="validate manifests; exit non-zero on any error (CI gate)",
    )
    parser.parse_args()

    errors = collect_manifest_errors(REPO_ROOT)
    if errors:
        print("Plugin manifest validation FAILED:")
        for entry in errors:
            print(f"  - {entry}")
        return 1
    print("Plugin manifest validation OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
