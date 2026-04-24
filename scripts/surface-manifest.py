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
HARNESS_PATH = REPO_ROOT / "docs" / "harness-modes.md"
HARNESS_SPEC_PATH = REPO_ROOT / "docs" / "harness-spec.md"
HOST_ADAPTIVE_PATH = REPO_ROOT / "docs" / "host-adaptive-harness.md"

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
PLATFORM_HARNESS_BEGIN = "<!-- SURFACE_MANIFEST_PLATFORM_HARNESS:BEGIN -->"
PLATFORM_HARNESS_END = "<!-- SURFACE_MANIFEST_PLATFORM_HARNESS:END -->"
INDEX_RELEASE_BEGIN = "<!-- SURFACE_MANIFEST_INDEX_RELEASE:BEGIN -->"
INDEX_RELEASE_END = "<!-- SURFACE_MANIFEST_INDEX_RELEASE:END -->"
HARNESS_OVERVIEW_BEGIN = "<!-- SURFACE_MANIFEST_HARNESS_OVERVIEW:BEGIN -->"
HARNESS_OVERVIEW_END = "<!-- SURFACE_MANIFEST_HARNESS_OVERVIEW:END -->"
HARNESS_DETAILS_BEGIN = "<!-- SURFACE_MANIFEST_HARNESS_DETAILS:BEGIN -->"
HARNESS_DETAILS_END = "<!-- SURFACE_MANIFEST_HARNESS_DETAILS:END -->"
HARNESS_SPEC_OVERVIEW_BEGIN = "<!-- SURFACE_MANIFEST_HARNESS_SPEC_OVERVIEW:BEGIN -->"
HARNESS_SPEC_OVERVIEW_END = "<!-- SURFACE_MANIFEST_HARNESS_SPEC_OVERVIEW:END -->"
HARNESS_SPEC_CONTRACTS_BEGIN = "<!-- SURFACE_MANIFEST_HARNESS_SPEC_CONTRACTS:BEGIN -->"
HARNESS_SPEC_CONTRACTS_END = "<!-- SURFACE_MANIFEST_HARNESS_SPEC_CONTRACTS:END -->"
HOST_ADAPTER_SUMMARY_BEGIN = "<!-- SURFACE_MANIFEST_HOST_ADAPTER_SUMMARY:BEGIN -->"
HOST_ADAPTER_SUMMARY_END = "<!-- SURFACE_MANIFEST_HOST_ADAPTER_SUMMARY:END -->"
HOST_ADAPTER_GUIDANCE_BEGIN = "<!-- SURFACE_MANIFEST_HOST_ADAPTER_GUIDANCE:BEGIN -->"
HOST_ADAPTER_GUIDANCE_END = "<!-- SURFACE_MANIFEST_HOST_ADAPTER_GUIDANCE:END -->"


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


def render_platform_harness_summary(manifest: dict) -> str:
    harness = manifest["harness_modes"]
    policy = harness["communication_policy"]
    mode_names = ", ".join(f"`{mode['name']}`" for mode in harness["modes"])
    handoff_schema = manifest["harness_artifacts"]["schemas"][0]
    return "\n".join(
        [
            f"- Default communication pattern: `{policy['default_pattern']}`",
            f"- Live bidirectional agent chat: `{policy['live_bidirectional_agent_chat']}`",
            f"- Planner -> builder delegation: `{policy['planner_to_builder_delegation']}`",
            f"- Builder -> planner escalation: `{policy['builder_to_planner_escalation']}`",
            f"- Canonical harness modes: {mode_names}",
            "- Runtime resources: `codelens://harness/modes`, `codelens://harness/spec`",
            f"- Handoff schema resource: `{handoff_schema['runtime_resource']}`",
        ]
    )


def render_index_release(manifest: dict) -> str:
    return "\n".join(
        [
            "- [Latest GitHub Release](https://github.com/mupozg823/codelens-mcp-plugin/releases/latest)",
            "- [All tagged releases](https://github.com/mupozg823/codelens-mcp-plugin/releases)",
            "- [Repository README](https://github.com/mupozg823/codelens-mcp-plugin/blob/main/README.md)",
            "- [Current source tree](https://github.com/mupozg823/codelens-mcp-plugin)",
        ]
    )


def render_harness_overview(manifest: dict) -> str:
    harness = manifest["harness_modes"]
    policy = harness["communication_policy"]
    return "\n".join(
        [
            f"- Schema: `{harness['schema_version']}`",
            f"- Default communication pattern: `{policy['default_pattern']}`",
            f"- Live bidirectional agent chat: `{policy['live_bidirectional_agent_chat']}`",
            f"- Planner -> builder delegation: `{policy['planner_to_builder_delegation']}`",
            f"- Builder -> planner escalation: `{policy['builder_to_planner_escalation']}`",
            f"- Shared substrate: `{policy['shared_substrate']}`",
            "- Runtime resource: `codelens://harness/modes`",
        ]
    )


def render_harness_details(manifest: dict) -> str:
    sections: list[str] = []
    for mode in manifest["harness_modes"]["modes"]:
        sections.extend(
            [
                f"### `{mode['name']}`",
                "",
                mode["purpose"],
                "",
                f"- Best fit: {mode['best_fit']}",
                f"- Communication pattern: `{mode['communication_pattern']}`",
                f"- Mutation policy: {mode['mutation_policy']}",
                f"- Transport: `{mode['topology']['transport']}`",
                f"- Daemon shape: `{mode['topology']['daemon_shape']}`",
            ]
        )
        ports = mode["topology"].get("recommended_ports", [])
        sections.append(
            "- Recommended ports: "
            + (", ".join(f"`{port}`" for port in ports) if ports else "none")
        )
        sections.append("- Roles:")
        for role in mode["roles"]:
            profiles = ", ".join(
                f"`{profile['name']}` ({profile['tool_count']})"
                for profile in role["profiles"]
            )
            sections.append(
                f"  - `{role['role']}`: {profiles}; mutate=`{str(role['can_mutate']).lower()}`; {role['responsibility']}"
            )
        sections.append("- Recommended flow:")
        for step in mode["recommended_flow"]:
            sections.append(f"  - `{step}`")
        sections.append("- Recommended audits:")
        for audit in mode["recommended_audits"]:
            sections.append(f"  - {audit}")
        sections.append("")
    return "\n".join(sections).rstrip()


def render_harness_spec_overview(manifest: dict) -> str:
    spec = manifest["harness_spec"]
    defaults = spec["defaults"]
    ttl = defaults["ttl_policy"]
    handoff_schema = manifest["harness_artifacts"]["schemas"][0]
    return "\n".join(
        [
            f"- Schema: `{spec['schema_version']}`",
            f"- Audit mode: `{defaults['audit_mode']}`",
            f"- Adds new runtime hard blocks: `{str(defaults['hard_blocks_added_by_spec']).lower()}`",
            f"- Recommended transport: `{defaults['recommended_transport']}`",
            f"- Preferred communication pattern: `{defaults['preferred_communication_pattern']}`",
            f"- TTL strategy: `{ttl['strategy']}`",
            f"- TTL default/max: `{ttl['default_secs']}` / `{ttl['max_secs']}` seconds",
            f"- Explicit release preferred: `{str(ttl['explicit_release_preferred']).lower()}`",
            "- Runtime resource: `codelens://harness/spec`",
            f"- Handoff artifact schema: `{handoff_schema['runtime_resource']}` ({handoff_schema['schema_version']})",
        ]
    )


def _render_contract_sequence(contract: dict) -> list[str]:
    if "preflight_sequence" in contract:
        title = "Preflight Sequence"
        sequence = contract["preflight_sequence"]
    elif "read_sequence" in contract:
        title = "Read Sequence"
        sequence = contract["read_sequence"]
    else:
        title = "Analysis Sequence"
        sequence = contract["analysis_sequence"]

    lines = [f"**{title}**"]
    for step in sequence:
        lines.append(
            f"- {step['order']}. `{step['tool']}`"
            f" | required=`{str(step['required']).lower()}`"
            f" | when: {step['when']}"
            f" | purpose: {step['purpose']}"
        )
    return lines


def render_harness_spec_contracts(manifest: dict) -> str:
    sections: list[str] = []
    for contract in manifest["harness_spec"]["contracts"]:
        sections.extend(
            [
                f"### `{contract['name']}`",
                "",
                f"- Mode: `{contract['mode']}`",
                f"- Intent: {contract['intent']}",
                "- Roles:",
            ]
        )
        for role in contract["roles"]:
            profiles = ", ".join(
                f"`{profile['name']}` ({profile['tool_count']})"
                for profile in role["profiles"]
            )
            sections.append(
                f"  - `{role['role']}`: {profiles}; mutate=`{str(role['can_mutate']).lower()}`; {role['responsibility']}"
            )

        sections.append("")
        sections.extend(_render_contract_sequence(contract))

        if "coordination_discipline" in contract:
            coordination = contract["coordination_discipline"]
            sections.extend(
                [
                    "",
                    "**Coordination Discipline**",
                    f"- Required for: {coordination['required_for']}",
                ]
            )
            for step in coordination["steps"]:
                sections.append(
                    f"- {step['order']}. `{step['tool']}`"
                    f" | required=`{str(step['required']).lower()}`"
                    f" | when: {step['when']}"
                    f" | purpose: {step['purpose']}"
                )
            ttl = coordination["ttl_policy"]
            sections.append(
                f"- TTL policy: `{ttl['strategy']}` | default/max=`{ttl['default_secs']}`/`{ttl['max_secs']}` | same TTL for registration and claims=`{str(ttl['same_ttl_for_registration_and_claims']).lower()}`"
            )

        if "mutation_execution" in contract:
            execution = contract["mutation_execution"]
            sections.extend(
                [
                    "",
                    "**Mutation Execution**",
                    "- Step order: "
                    + ", ".join(f"`{step}`" for step in execution["step_order"]),
                ]
            )
            for note in execution["notes"]:
                sections.append(f"- Note: {note}")

        if "resource_handoff" in contract:
            handoff = contract["resource_handoff"]
            sections.extend(
                [
                    "",
                    "**Resource Handoff**",
                    f"- Summary resource pattern: `{handoff['summary_resource_pattern']}`",
                    f"- Section access pattern: `{handoff['section_access_pattern']}`",
                    f"- Metrics tool: `{handoff['metrics_tool']}`",
                ]
            )

        sections.extend(["", "**Gates**"])
        for gate in contract["gates"]:
            line = (
                f"- condition: `{gate['condition']}`"
                f" | action: `{gate['action']}`"
                f" | reason: {gate['reason']}"
            )
            required_tools = gate.get("required_tools")
            if required_tools:
                line += " | required tools: " + ", ".join(f"`{tool}`" for tool in required_tools)
            sections.append(line)

        sections.extend(["", "**Audit Hooks**"])
        for key, value in contract["audits"].items():
            sections.append(f"- `{key}`: `{value}`")

        artifact = contract["handoff_artifact_template"]
        sections.extend(
            [
                "",
                "**Handoff Artifact Template**",
                f"- Name: `{artifact['name']}`",
                f"- Format: `{artifact['format']}`",
                "- Required fields: " + ", ".join(f"`{field}`" for field in artifact["required_fields"]),
                "- Example skeleton:",
                "```json",
                json.dumps(artifact["example"], indent=2),
                "```",
                "",
            ]
        )
    return "\n".join(sections).rstrip()


def render_host_adapter_summary(manifest: dict) -> str:
    sections = [
        "## Generated Host Runtime Snapshot",
        "",
        "Generated from the canonical surface manifest. Runtime resources remain the authoritative source when the doc and live server differ.",
        "",
    ]
    for host in manifest["host_adapters"]["hosts"]:
        sections.extend(
            [
                f"### `{host['name']}`",
                "",
                f"- Resource: `{host['resource_uri']}`",
                f"- Best fit: {host['best_fit']}",
                "- Recommended modes: "
                + ", ".join(f"`{mode}`" for mode in host["recommended_modes"]),
                "- Preferred profiles: "
                + ", ".join(f"`{profile}`" for profile in host["preferred_profiles"]),
                f"- Default compiled overlay: profile=`{host['default_profile']}`, task_overlay=`{host['default_task_overlay']}`",
                "- Primary bootstrap sequence: "
                + " -> ".join(f"`{step}`" for step in host["primary_bootstrap_sequence"]),
                "- Compiler targets: "
                + ", ".join(f"`{target}`" for target in host["compiler_targets"]),
                "",
            ]
        )
    return "\n".join(sections).rstrip()


def render_host_adapter_guidance(manifest: dict) -> str:
    sections = [
        "Generated from the canonical surface manifest. Use this block as the default operator guidance when the prose below is stale.",
        "",
    ]
    for host in manifest["host_adapters"]["hosts"]:
        routing_defaults = host.get("routing_defaults", {})
        routing_summary = ", ".join(
            f"`{key}={value}`" for key, value in routing_defaults.items()
        )
        sections.extend(
            [
                f"### `{host['name']}`",
                "",
                f"- Best fit: {host['best_fit']}",
                "- Recommended CodeLens modes: "
                + ", ".join(f"`{mode}`" for mode in host["recommended_modes"]),
                "- Preferred profiles: "
                + ", ".join(f"`{profile}`" for profile in host["preferred_profiles"]),
                "- Native host primitives: "
                + ", ".join(f"`{item}`" for item in host["native_primitives"]),
                "- Use CodeLens for: "
                + "; ".join(host["preferred_codelens_use"]),
                "- Avoid: "
                + "; ".join(host["avoid"]),
                "- Routing defaults: " + routing_summary,
                "",
            ]
        )
    return "\n".join(sections).rstrip()


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
    platform = replace_block(
        platform,
        PLATFORM_HARNESS_BEGIN,
        PLATFORM_HARNESS_END,
        render_platform_harness_summary(manifest),
    )

    index = INDEX_PATH.read_text(encoding="utf-8")
    index = replace_block(
        index,
        INDEX_RELEASE_BEGIN,
        INDEX_RELEASE_END,
        render_index_release(manifest),
    )

    harness = HARNESS_PATH.read_text(encoding="utf-8")
    harness = replace_block(
        harness,
        HARNESS_OVERVIEW_BEGIN,
        HARNESS_OVERVIEW_END,
        render_harness_overview(manifest),
    )
    harness = replace_block(
        harness,
        HARNESS_DETAILS_BEGIN,
        HARNESS_DETAILS_END,
        render_harness_details(manifest),
    )

    harness_spec = HARNESS_SPEC_PATH.read_text(encoding="utf-8")
    harness_spec = replace_block(
        harness_spec,
        HARNESS_SPEC_OVERVIEW_BEGIN,
        HARNESS_SPEC_OVERVIEW_END,
        render_harness_spec_overview(manifest),
    )
    harness_spec = replace_block(
        harness_spec,
        HARNESS_SPEC_CONTRACTS_BEGIN,
        HARNESS_SPEC_CONTRACTS_END,
        render_harness_spec_contracts(manifest),
    )

    host_adaptive = HOST_ADAPTIVE_PATH.read_text(encoding="utf-8")
    host_adaptive = replace_block(
        host_adaptive,
        HOST_ADAPTER_SUMMARY_BEGIN,
        HOST_ADAPTER_SUMMARY_END,
        render_host_adapter_summary(manifest),
    )
    host_adaptive = replace_block(
        host_adaptive,
        HOST_ADAPTER_GUIDANCE_BEGIN,
        HOST_ADAPTER_GUIDANCE_END,
        render_host_adapter_guidance(manifest),
    )

    return {
        MANIFEST_PATH: manifest_text,
        README_PATH: readme,
        ARCH_PATH: arch,
        PLATFORM_PATH: platform,
        INDEX_PATH: index,
        HARNESS_PATH: harness,
        HARNESS_SPEC_PATH: harness_spec,
        HOST_ADAPTIVE_PATH: host_adaptive,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--write",
        action="store_true",
        help="write docs/generated/surface-manifest.json and refresh generated doc blocks",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="check for drift without writing; explicit alias for the default mode",
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
