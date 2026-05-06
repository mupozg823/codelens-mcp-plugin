#!/usr/bin/env python3
"""Current user-facing docs must not advertise removed workflow aliases."""

from __future__ import annotations

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
CURRENT_DOCS = (
    "README.md",
    "docs/architecture.md",
    "docs/index.md",
    "docs/platform-setup.md",
    "docs/harness-modes.md",
    "docs/harness-spec.md",
    "docs/host-adaptive-harness.md",
)
CURRENT_RUNTIME_SOURCES = (
    "crates/codelens-mcp/src/tools/composite.rs",
    "crates/codelens-mcp/src/tools/workflows.rs",
    "crates/codelens-mcp/src/tools/reports/context_reports.rs",
    "crates/codelens-mcp/src/tools/reports/mod.rs",
    "crates/codelens-mcp/src/tools/reports/impact_reports/refactor.rs",
    "crates/codelens-mcp/src/tools/report_jobs.rs",
)
REMOVED_WORKFLOW_ALIASES = (
    "explain_code_flow",
    "find_minimal_context_for_change",
    "summarize_symbol_impact",
)


def test_current_docs_do_not_advertise_removed_workflow_aliases() -> None:
    offenders: list[str] = []
    for doc in CURRENT_DOCS:
        path = REPO_ROOT / doc
        text = path.read_text(encoding="utf-8")
        for alias in REMOVED_WORKFLOW_ALIASES:
            if alias in text:
                offenders.append(f"{doc}: {alias}")
    assert not offenders, "removed workflow aliases leaked into current docs:\n  " + "\n  ".join(
        offenders
    )


def test_current_runtime_sources_do_not_export_removed_workflow_aliases() -> None:
    offenders: list[str] = []
    for source in CURRENT_RUNTIME_SOURCES:
        path = REPO_ROOT / source
        text = path.read_text(encoding="utf-8")
        for alias in REMOVED_WORKFLOW_ALIASES:
            if alias in text:
                offenders.append(f"{source}: {alias}")
    assert (
        not offenders
    ), "removed workflow aliases leaked into current runtime sources:\n  " + "\n  ".join(
        offenders
    )


def main() -> int:
    failures: list[str] = []
    try:
        test_current_docs_do_not_advertise_removed_workflow_aliases()
        print("PASS  current_docs_do_not_advertise_removed_workflow_aliases")
    except AssertionError as exc:
        print(f"FAIL  current_docs_do_not_advertise_removed_workflow_aliases: {exc}")
        failures.append("current_docs_do_not_advertise_removed_workflow_aliases")
    try:
        test_current_runtime_sources_do_not_export_removed_workflow_aliases()
        print("PASS  current_runtime_sources_do_not_export_removed_workflow_aliases")
    except AssertionError as exc:
        print(f"FAIL  current_runtime_sources_do_not_export_removed_workflow_aliases: {exc}")
        failures.append("current_runtime_sources_do_not_export_removed_workflow_aliases")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
