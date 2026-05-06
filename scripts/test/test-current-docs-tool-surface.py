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


def main() -> int:
    try:
        test_current_docs_do_not_advertise_removed_workflow_aliases()
        print("PASS  current_docs_do_not_advertise_removed_workflow_aliases")
        return 0
    except AssertionError as exc:
        print(f"FAIL  current_docs_do_not_advertise_removed_workflow_aliases: {exc}")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
