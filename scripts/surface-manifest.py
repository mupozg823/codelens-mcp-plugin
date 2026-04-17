#!/usr/bin/env python3
"""Generate or check the canonical surface manifest and generated doc blocks."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
MANIFEST_PATH = REPO_ROOT / "docs" / "generated" / "surface-manifest.json"
README_PATH = REPO_ROOT / "README.md"
ARCH_PATH = REPO_ROOT / "docs" / "architecture.md"
PLATFORM_PATH = REPO_ROOT / "docs" / "platform-setup.md"
INDEX_PATH = REPO_ROOT / "docs" / "index.md"

README_SNAPSHOT_BEGIN = "<!-- SURFACE_MANIFEST_README_SNAPSHOT:BEGIN -->"
README_SNAPSHOT_END = "<!-- SURFACE_MANIFEST_README_SNAPSHOT:END -->"
README_LANG_BEGIN = "<!-- SURFACE_MANIFEST_README_LANGUAGES:BEGIN -->"
README_LANG_END = "<!-- SURFACE_MANIFEST_README_LANGUAGES:END -->"
ARCH_SNAPSHOT_BEGIN = "<!-- SURFACE_MANIFEST_ARCHITECTURE_SNAPSHOT:BEGIN -->"
ARCH_SNAPSHOT_END = "<!-- SURFACE_MANIFEST_ARCHITECTURE_SNAPSHOT:END -->"
ARCH_LANG_BEGIN = "<!-- SURFACE_MANIFEST_ARCHITECTURE_LANGUAGES:BEGIN -->"
ARCH_LANG_END = "<!-- SURFACE_MANIFEST_ARCHITECTURE_LANGUAGES:END -->"
PLATFORM_SURFACES_BEGIN = "<!-- SURFACE_MANIFEST_PLATFORM_SURFACES:BEGIN -->"
PLATFORM_SURFACES_END = "<!-- SURFACE_MANIFEST_PLATFORM_SURFACES:END -->"
INDEX_RELEASE_BEGIN = "<!-- SURFACE_MANIFEST_INDEX_RELEASE:BEGIN -->"
INDEX_RELEASE_END = "<!-- SURFACE_MANIFEST_INDEX_RELEASE:END -->"


def load_manifest() -> dict:
    cmd = [
        "cargo",
        "run",
        "-q",
        "-p",
        "codelens-mcp",
        "--features",
        "http",
        "--",
        "--print-surface-manifest",
    ]
    proc = subprocess.run(
        cmd,
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr)
        raise SystemExit(proc.returncode)
    return json.loads(proc.stdout)


def replace_block(text: str, begin: str, end: str, content: str) -> str:
    start = text.find(begin)
    finish = text.find(end)
    if start == -1 or finish == -1 or finish < start:
        raise SystemExit(f"missing marker pair: {begin} .. {end}")
    finish += len(end)
    replacement = f"{begin}\n{content}\n{end}"
    return text[:start] + replacement + text[finish:]


def profile_counts(manifest: dict) -> str:
    profiles = manifest["surfaces"]["profiles"]
    return ", ".join(
        f"`{profile['name']}` ({profile['tool_count']})" for profile in profiles
    )


def preset_counts(manifest: dict) -> str:
    presets = manifest["surfaces"]["presets"]
    return ", ".join(
        f"`{preset['name']}` ({preset['tool_count']})" for preset in presets
    )


def render_readme_snapshot(manifest: dict) -> str:
    summary = manifest["summary"]
    workspace = manifest["workspace"]
    return "\n".join(
        [
            "## Surface Snapshot",
            "",
            f"- Workspace version: `{workspace['version']}`",
            f"- Workspace members: `{workspace['member_count']}` ({', '.join(f'`{member}`' for member in workspace['members'])})",
            f"- Registered tool definitions: `{summary['registered_tool_definitions']}`",
            f"- Tool output schemas: `{summary['tool_output_schemas']['declared']} / {summary['tool_output_schemas']['total']}`",
            f"- Supported language families: `{summary['supported_language_families']}` across `{summary['supported_extensions']}` extensions",
            f"- Profiles: {profile_counts(manifest)}",
            f"- Presets: {preset_counts(manifest)}",
            "- Canonical manifest: [`docs/generated/surface-manifest.json`](docs/generated/surface-manifest.json)",
        ]
    )


def render_architecture_snapshot(manifest: dict) -> str:
    summary = manifest["summary"]
    workspace = manifest["workspace"]
    return "\n".join(
        [
            f"- Workspace version: `{workspace['version']}`",
            f"- Workspace members: `{workspace['member_count']}` (`{'`, `'.join(workspace['members'])}`)",
            f"- Registered tool definitions in source: `{summary['registered_tool_definitions']}`",
            f"- Tool output schemas in source: `{summary['tool_output_schemas']['declared']} / {summary['tool_output_schemas']['total']}`",
            f"- Supported language families: `{summary['supported_language_families']}` across `{summary['supported_extensions']}` extensions",
            "- Canonical manifest: [`docs/generated/surface-manifest.json`](generated/surface-manifest.json)",
        ]
    )


def render_platform_surfaces(manifest: dict) -> str:
    workspace = manifest["workspace"]
    presets = manifest["surfaces"]["presets"]
    profiles = manifest["surfaces"]["profiles"]
    return "\n".join(
        [
            f"- Workspace version: `{workspace['version']}`",
            "- Presets: "
            + ", ".join(f"`{p['name']}` ({p['tool_count']})" for p in presets),
            "- Profiles: "
            + ", ".join(f"`{p['name']}` ({p['tool_count']})" for p in profiles),
            "- Canonical manifest: [`docs/generated/surface-manifest.json`](generated/surface-manifest.json)",
        ]
    )


def render_index_release(manifest: dict) -> str:
    version = manifest["workspace"]["version"]
    return "\n".join(
        [
            f"- [GitHub Release v{version}](https://github.com/mupozg823/codelens-mcp-plugin/releases/tag/v{version})",
            "- [Repository README](https://github.com/mupozg823/codelens-mcp-plugin/blob/main/README.md)",
            "- [Current source tree](https://github.com/mupozg823/codelens-mcp-plugin)",
        ]
    )


def render_language_block(manifest: dict, link_path: str) -> str:
    families = manifest["languages"]["families"]
    names = ", ".join(family["display_name"] for family in families)
    import_capable = [
        family["display_name"] for family in families if family["supports_imports"]
    ]
    return "\n".join(
        [
            f"Canonical parser families ({manifest['languages']['language_family_count']}): {names}",
            "",
            f"Import-graph capable families: {', '.join(import_capable)}",
            "",
            f"The canonical family/extension inventory is generated from `codelens_engine::lang_registry` and published in [`docs/generated/surface-manifest.json`]({link_path}).",
        ]
    )


def expected_files(manifest: dict) -> dict[Path, str]:
    manifest_text = json.dumps(manifest, indent=2) + "\n"

    readme = README_PATH.read_text(encoding="utf-8")
    readme = replace_block(
        readme,
        README_SNAPSHOT_BEGIN,
        README_SNAPSHOT_END,
        render_readme_snapshot(manifest),
    )
    readme = replace_block(
        readme,
        README_LANG_BEGIN,
        README_LANG_END,
        render_language_block(manifest, "docs/generated/surface-manifest.json"),
    )

    arch = ARCH_PATH.read_text(encoding="utf-8")
    arch = replace_block(
        arch,
        ARCH_SNAPSHOT_BEGIN,
        ARCH_SNAPSHOT_END,
        render_architecture_snapshot(manifest),
    )
    arch = replace_block(
        arch,
        ARCH_LANG_BEGIN,
        ARCH_LANG_END,
        render_language_block(manifest, "generated/surface-manifest.json"),
    )

    platform = PLATFORM_PATH.read_text(encoding="utf-8")
    platform = replace_block(
        platform,
        PLATFORM_SURFACES_BEGIN,
        PLATFORM_SURFACES_END,
        render_platform_surfaces(manifest),
    )

    index = INDEX_PATH.read_text(encoding="utf-8")
    index = replace_block(
        index,
        INDEX_RELEASE_BEGIN,
        INDEX_RELEASE_END,
        render_index_release(manifest),
    )

    return {
        MANIFEST_PATH: manifest_text,
        README_PATH: readme,
        ARCH_PATH: arch,
        PLATFORM_PATH: platform,
        INDEX_PATH: index,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--write",
        action="store_true",
        help="write docs/generated/surface-manifest.json and refresh generated doc blocks",
    )
    args = parser.parse_args()

    manifest = load_manifest()
    expected = expected_files(manifest)

    drifted: list[Path] = []
    for path, content in expected.items():
        current = path.read_text(encoding="utf-8") if path.exists() else None
        if current != content:
            drifted.append(path)
            if args.write:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content, encoding="utf-8")

    if drifted and not args.write:
        print("surface manifest drift detected:")
        for path in drifted:
            print(f"- {path.relative_to(REPO_ROOT)}")
        raise SystemExit(1)

    if args.write:
        print("surface manifest refreshed:")
        for path in drifted:
            print(f"- {path.relative_to(REPO_ROOT)}")


if __name__ == "__main__":
    main()
