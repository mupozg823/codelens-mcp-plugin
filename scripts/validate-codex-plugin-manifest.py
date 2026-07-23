#!/usr/bin/env python3
"""Validate the repo-owned CodeLens package for Codex."""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import TypeAlias

REPO_ROOT = Path(__file__).resolve().parents[1]
PLUGIN_MANIFEST = Path("plugins/codelens/.codex-plugin/plugin.json")
MARKETPLACE_MANIFEST = Path(".agents/plugins/marketplace.json")
SKILL_MANIFEST = Path("plugins/codelens/skills/codelens/SKILL.md")
SKILL_AGENT_MANIFEST = Path("plugins/codelens/skills/codelens/agents/openai.yaml")
JsonScalar: TypeAlias = str | int | float | bool | None
JsonValue: TypeAlias = JsonScalar | list["JsonValue"] | dict[str, "JsonValue"]
JsonObject: TypeAlias = dict[str, JsonValue]
EXPECTED_MARKETPLACE_PLUGINS: list[JsonValue] = [
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
]
CODELENS_MCP_DEPENDENCY = re.compile(
    r'(?m)^\s*-\s+type:\s*["\']mcp["\']\s*$'
    r'\n\s+value:\s*["\']codelens["\']\s*$'
)
MCP_ENDPOINT_METADATA = re.compile(r"(?m)^\s+(?:transport|url):")
FRONTMATTER_KEY = re.compile(r"(?m)^([a-z][a-z0-9_-]*):")
CANONICAL_SKILL_NAME = re.compile(r"(?m)^name:\s*codelens\s*$")
CANONICAL_DEFAULT_PROMPT = re.compile(r"(?m)^\s+default_prompt:\s*[\"'].*\$codelens\b")


def _workspace_version(repo_root: Path) -> str | None:
    cargo_manifest = repo_root / "Cargo.toml"
    if not cargo_manifest.is_file():
        return None
    in_workspace_package = False
    for line in cargo_manifest.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if stripped.startswith("["):
            in_workspace_package = stripped == "[workspace.package]"
        elif in_workspace_package and stripped.startswith("version"):
            return stripped.partition("=")[2].strip().strip('"')
    return None


def _load_json_object(
    path: Path,
    label: str,
    errors: list[str],
) -> JsonObject | None:
    if not path.is_file():
        errors.append(f"{label}: missing")
        return None
    try:
        payload: JsonValue = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        errors.append(f"{label}: invalid JSON ({exc})")
        return None
    if isinstance(payload, dict):
        return payload
    errors.append(f"{label}: root must be an object")
    return None


def collect_codex_plugin_errors(repo_root: Path) -> list[str]:
    """Return deterministic Codex package contract errors."""
    errors: list[str] = []
    plugin = _load_json_object(
        repo_root / PLUGIN_MANIFEST,
        PLUGIN_MANIFEST.as_posix(),
        errors,
    )
    marketplace = _load_json_object(
        repo_root / MARKETPLACE_MANIFEST,
        MARKETPLACE_MANIFEST.as_posix(),
        errors,
    )
    for path in (SKILL_MANIFEST, SKILL_AGENT_MANIFEST):
        if not (repo_root / path).is_file():
            errors.append(f"{path.as_posix()}: missing")
    if (plugin is not None and "mcpServers" in plugin) or (
        repo_root / "plugins/codelens/.mcp.json"
    ).exists():
        errors.append(
            "Codex package must reuse the host-level MCP registration; "
            "remove mcpServers and .mcp.json"
        )
    if plugin is not None:
        expected_version = _workspace_version(repo_root)
        actual_version = plugin.get("version")
        if (
            expected_version is not None
            and isinstance(actual_version, str)
            and actual_version != expected_version
        ):
            errors.append(
                f"Codex plugin version {actual_version} "
                f"must equal workspace version {expected_version}"
            )
    if (
        marketplace is not None
        and marketplace.get("plugins") != EXPECTED_MARKETPLACE_PLUGINS
    ):
        errors.append("Codex marketplace source must be local ./plugins/codelens")
    agent_path = repo_root / SKILL_AGENT_MANIFEST
    if agent_path.is_file():
        agent_text = agent_path.read_text(encoding="utf-8")
        if CODELENS_MCP_DEPENDENCY.search(agent_text) is None:
            errors.append("Codex skill must depend on the existing codelens MCP")
        if MCP_ENDPOINT_METADATA.search(agent_text) is not None:
            errors.append(
                "Codex skill MCP dependency must not declare transport or url"
            )
        if CANONICAL_DEFAULT_PROMPT.search(agent_text) is None:
            errors.append("Codex skill canonical explicit invocation is $codelens")
    skill_path = repo_root / SKILL_MANIFEST
    if skill_path.is_file():
        skill_text = skill_path.read_text(encoding="utf-8")
        frontmatter_end = skill_text.find("\n---", 4)
        if not skill_text.startswith("---\n") or frontmatter_end == -1:
            errors.append("Codex skill must contain closed YAML frontmatter")
        else:
            frontmatter = skill_text[4:frontmatter_end]
            fields = set(FRONTMATTER_KEY.findall(frontmatter))
            if fields != {"name", "description"}:
                errors.append(
                    "Codex skill frontmatter fields must be name and description only"
                )
            if CANONICAL_SKILL_NAME.search(frontmatter) is None:
                errors.append("Codex skill canonical explicit invocation is $codelens")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=REPO_ROOT,
        help="repository root to validate",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="exit non-zero when validation errors are present",
    )
    args = parser.parse_args()

    errors = collect_codex_plugin_errors(args.repo_root.resolve())
    if errors:
        print("Codex plugin manifest validation FAILED:")
        for error in errors:
            print(f"  - {error}")
        return 1
    print("Codex plugin manifest validation OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
