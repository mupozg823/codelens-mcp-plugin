#!/usr/bin/env python3
"""Contract tests for scripts/validate-plugin-manifest.py."""
from __future__ import annotations

import importlib.util
import json
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
_SPEC = importlib.util.spec_from_file_location(
    "validate_plugin_manifest", REPO_ROOT / "scripts" / "validate-plugin-manifest.py"
)
_MOD = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_MOD)
collect_manifest_errors = _MOD.collect_manifest_errors


VALID_PLUGIN = {
    "name": "codelens",
    "version": "1.0.0",
    "description": "d",
    "mcpServers": {"codelens": {"command": "codelens-mcp"}},
    "skills": "./skills/",
    "agents": "./agents/",
}
VALID_MARKET = {
    "name": "codelens",
    "owner": {"name": "x"},
    "plugins": [{"name": "codelens", "source": "./", "description": "d"}],
}


def _write(root: Path, plugin, market) -> None:
    cp = root / ".claude-plugin"
    cp.mkdir(parents=True, exist_ok=True)
    (root / "skills").mkdir(exist_ok=True)
    (root / "skills" / "x").write_text("x", encoding="utf-8")
    (root / "agents").mkdir(exist_ok=True)
    (root / "agents" / "a.md").write_text("a", encoding="utf-8")
    if plugin is not None:
        text = plugin if isinstance(plugin, str) else json.dumps(plugin)
        (cp / "plugin.json").write_text(text, encoding="utf-8")
    if market is not None:
        text = market if isinstance(market, str) else json.dumps(market)
        (cp / "marketplace.json").write_text(text, encoding="utf-8")


def test_valid_manifests_pass() -> None:
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        _write(root, VALID_PLUGIN, VALID_MARKET)
        assert collect_manifest_errors(root) == []


def test_missing_plugin_file_reported() -> None:
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        _write(root, None, VALID_MARKET)
        errs = collect_manifest_errors(root)
        assert any("plugin.json" in e and "missing" in e for e in errs)


def test_broken_json_reported() -> None:
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        _write(root, "{ not json", VALID_MARKET)
        errs = collect_manifest_errors(root)
        assert any("invalid JSON" in e for e in errs)


def test_missing_required_field_reported() -> None:
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        p = dict(VALID_PLUGIN)
        del p["mcpServers"]
        _write(root, p, VALID_MARKET)
        errs = collect_manifest_errors(root)
        assert any("mcpServers" in e for e in errs)


def test_bad_mcp_command_reported() -> None:
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        p = dict(VALID_PLUGIN)
        p["mcpServers"] = {"codelens": {"command": ""}}
        _write(root, p, VALID_MARKET)
        errs = collect_manifest_errors(root)
        assert any("command" in e for e in errs)


def test_dangling_skills_path_reported() -> None:
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        p = dict(VALID_PLUGIN)
        p["skills"] = "./nope/"
        _write(root, p, VALID_MARKET)
        errs = collect_manifest_errors(root)
        assert any("skills" in e and "nope" in e for e in errs)


def test_marketplace_name_mismatch_reported() -> None:
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        m = json.loads(json.dumps(VALID_MARKET))
        m["plugins"][0]["name"] = "wrong"
        _write(root, VALID_PLUGIN, m)
        errs = collect_manifest_errors(root)
        assert any("match" in e.lower() for e in errs)


def test_empty_plugins_array_reported() -> None:
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        m = json.loads(json.dumps(VALID_MARKET))
        m["plugins"] = []
        _write(root, VALID_PLUGIN, m)
        errs = collect_manifest_errors(root)
        assert any("plugins" in e for e in errs)


def main() -> int:
    failures: list[str] = []
    tests = [
        test_valid_manifests_pass,
        test_missing_plugin_file_reported,
        test_broken_json_reported,
        test_missing_required_field_reported,
        test_bad_mcp_command_reported,
        test_dangling_skills_path_reported,
        test_marketplace_name_mismatch_reported,
        test_empty_plugins_array_reported,
    ]
    for t in tests:
        try:
            t()
            print(f"PASS  {t.__name__}")
        except AssertionError as exc:
            print(f"FAIL  {t.__name__}: {exc}")
            failures.append(t.__name__)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
