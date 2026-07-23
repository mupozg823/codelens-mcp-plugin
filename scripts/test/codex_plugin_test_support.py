"""Shared fixtures for the Codex plugin manifest contract tests."""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
VALIDATOR = REPO_ROOT / "scripts" / "validate-codex-plugin-manifest.py"
PLUGIN_ROOT = Path("plugins/codelens")
PLUGIN_MANIFEST = PLUGIN_ROOT / ".codex-plugin/plugin.json"
MARKETPLACE_MANIFEST = Path(".agents/plugins/marketplace.json")
SKILL_ROOT = PLUGIN_ROOT / "skills/codelens"
SKILL_MANIFEST = SKILL_ROOT / "SKILL.md"
SKILL_AGENT_MANIFEST = SKILL_ROOT / "agents/openai.yaml"


def _write_valid_package(root: Path) -> None:
    plugin_root = root / PLUGIN_ROOT
    skill_root = root / SKILL_ROOT
    (plugin_root / ".codex-plugin").mkdir(parents=True)
    (skill_root / "agents").mkdir(parents=True)
    (root / ".agents" / "plugins").mkdir(parents=True)
    (root / "Cargo.toml").write_text(
        '[workspace.package]\nversion = "1.13.34"\n',
        encoding="utf-8",
    )
    (root / PLUGIN_MANIFEST).write_text(
        json.dumps(
            {
                "name": "codelens",
                "version": "1.13.34",
                "description": "CodeLens skill adapter for Codex.",
                "author": {"name": "mupozg823"},
                "skills": "./skills/",
                "interface": {
                    "displayName": "CodeLens",
                    "shortDescription": "Use CodeLens code intelligence in Codex.",
                    "longDescription": "Bind and query an existing CodeLens MCP server.",
                    "developerName": "mupozg823",
                    "category": "Developer Tools",
                    "capabilities": ["Interactive", "Read"],
                    "defaultPrompt": ["Use $codelens to inspect this repository."],
                },
            }
        ),
        encoding="utf-8",
    )
    (root / MARKETPLACE_MANIFEST).write_text(
        json.dumps(
            {
                "name": "codelens",
                "interface": {"displayName": "CodeLens"},
                "plugins": [
                    {
                        "name": "codelens",
                        "source": {
                            "source": "local",
                            "path": "./plugins/codelens",
                        },
                        "policy": {
                            "installation": "AVAILABLE",
                            "authentication": "ON_INSTALL",
                        },
                        "category": "Developer Tools",
                    }
                ],
            }
        ),
        encoding="utf-8",
    )
    (root / SKILL_MANIFEST).write_text(
        "---\n"
        "name: codelens\n"
        "description: Use CodeLens for multi-file code analysis.\n"
        "---\n\n"
        "# CodeLens\n",
        encoding="utf-8",
    )
    (root / SKILL_AGENT_MANIFEST).write_text(
        "interface:\n"
        '  display_name: "CodeLens"\n'
        '  short_description: "Use CodeLens code intelligence"\n'
        '  default_prompt: "Use $codelens to inspect this repository."\n'
        "dependencies:\n"
        "  tools:\n"
        '    - type: "mcp"\n'
        '      value: "codelens"\n'
        '      description: "Existing CodeLens MCP server"\n',
        encoding="utf-8",
    )


@contextmanager
def valid_package() -> Iterator[Path]:
    with tempfile.TemporaryDirectory() as temp_dir:
        root = Path(temp_dir)
        _write_valid_package(root)
        yield root


def run_validator(root: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(VALIDATOR), "--repo-root", str(root), "--check"],
        check=False,
        capture_output=True,
        text=True,
    )
