#!/usr/bin/env python3
"""Contract tests for scripts/validate-codex-plugin-manifest.py."""

from __future__ import annotations

import json
from codex_plugin_test_support import (
    MARKETPLACE_MANIFEST,
    PLUGIN_MANIFEST,
    PLUGIN_ROOT,
    REPO_ROOT,
    SKILL_AGENT_MANIFEST,
    SKILL_MANIFEST,
    run_validator,
    valid_package,
)


def test_valid_skill_only_codex_package_passes() -> None:
    with valid_package() as root:
        result = run_validator(root)

        assert result.returncode == 0, result.stdout + result.stderr
        assert "Codex plugin manifest validation OK" in result.stdout


def test_codex_package_rejects_duplicate_mcp_registration() -> None:
    with valid_package() as root:
        plugin_root = root / PLUGIN_ROOT
        manifest_path = root / PLUGIN_MANIFEST
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        manifest["mcpServers"] = {
            "codelens": {
                "type": "http",
                "url": "http://127.0.0.1:7838/mcp",
            }
        }
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        (plugin_root / ".mcp.json").write_text(
            '{"mcpServers":{"codelens":{}}}',
            encoding="utf-8",
        )

        result = run_validator(root)

        assert result.returncode == 1
        assert "must reuse the host-level MCP registration" in result.stdout


def test_codex_marketplace_requires_nested_local_source() -> None:
    with valid_package() as root:
        marketplace_path = root / MARKETPLACE_MANIFEST
        marketplace = json.loads(marketplace_path.read_text(encoding="utf-8"))
        marketplace["plugins"][0]["source"] = {
            "source": "local",
            "path": "./",
        }
        marketplace_path.write_text(json.dumps(marketplace), encoding="utf-8")

        result = run_validator(root)

        assert result.returncode == 1
        assert "source must be local ./plugins/codelens" in result.stdout


def test_codex_skill_requires_existing_codelens_mcp_dependency() -> None:
    with valid_package() as root:
        agent_path = root / SKILL_AGENT_MANIFEST
        agent_path.write_text(
            "interface:\n"
            '  display_name: "CodeLens"\n'
            '  short_description: "Use CodeLens code intelligence"\n'
            '  default_prompt: "Use $codelens to inspect this repository."\n',
            encoding="utf-8",
        )

        result = run_validator(root)

        assert result.returncode == 1
        assert "must depend on the existing codelens MCP" in result.stdout


def test_codex_skill_dependency_rejects_endpoint_metadata() -> None:
    with valid_package() as root:
        agent_path = root / SKILL_AGENT_MANIFEST
        agent_path.write_text(
            agent_path.read_text(encoding="utf-8")
            + '      transport: "streamable_http"\n'
            + '      url: "http://127.0.0.1:7838/mcp"\n',
            encoding="utf-8",
        )

        result = run_validator(root)

        assert result.returncode == 1
        assert "must not declare transport or url" in result.stdout


def test_codex_plugin_version_tracks_workspace_version() -> None:
    with valid_package() as root:
        manifest_path = root / PLUGIN_MANIFEST
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        manifest["version"] = "0.1.0"
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")

        result = run_validator(root)

        assert result.returncode == 1
        assert "version 0.1.0 must equal workspace version 1.13.34" in result.stdout


def test_codex_skill_rejects_legacy_claude_frontmatter() -> None:
    with valid_package() as root:
        skill_path = root / SKILL_MANIFEST
        skill_path.write_text(
            "---\n"
            "name: codelens\n"
            "description: Use CodeLens for multi-file code analysis.\n"
            'trigger: "/codelens"\n'
            "tools: [prepare_harness_session]\n"
            "---\n\n"
            "# CodeLens\n",
            encoding="utf-8",
        )

        result = run_validator(root)

        assert result.returncode == 1
        assert "frontmatter fields must be name and description only" in result.stdout


def test_codex_package_requires_canonical_explicit_invocation() -> None:
    with valid_package() as root:
        skill_path = root / SKILL_MANIFEST
        skill_path.write_text(
            skill_path.read_text(encoding="utf-8").replace(
                "name: codelens",
                "name: codelens-review",
            ),
            encoding="utf-8",
        )
        agent_path = root / SKILL_AGENT_MANIFEST
        agent_path.write_text(
            agent_path.read_text(encoding="utf-8").replace(
                "$codelens",
                "$codelens-review",
            ),
            encoding="utf-8",
        )

        result = run_validator(root)

        assert result.returncode == 1
        assert "canonical explicit invocation is $codelens" in result.stdout


def test_repository_codex_package_passes() -> None:
    result = run_validator(REPO_ROOT)

    assert result.returncode == 0, result.stdout + result.stderr


def main() -> int:
    failures: list[str] = []
    tests = [
        test_valid_skill_only_codex_package_passes,
        test_codex_package_rejects_duplicate_mcp_registration,
        test_codex_marketplace_requires_nested_local_source,
        test_codex_skill_requires_existing_codelens_mcp_dependency,
        test_codex_skill_dependency_rejects_endpoint_metadata,
        test_codex_plugin_version_tracks_workspace_version,
        test_codex_skill_rejects_legacy_claude_frontmatter,
        test_codex_package_requires_canonical_explicit_invocation,
        test_repository_codex_package_passes,
    ]
    for test in tests:
        try:
            test()
            print(f"PASS  {test.__name__}")
        except AssertionError as exc:
            print(f"FAIL  {test.__name__}: {exc}")
            failures.append(test.__name__)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
