#!/usr/bin/env python3
"""Contract tests for the 3-way surface-drift report + description lint
in scripts/regen-tool-defs.py (PLAN_serena-alignment-p1 Phase 1).

Covers:
  - parse_dispatch_names(): both registration styles (match arms in
    tools/mod.rs `tool_registry!`, `.insert("name", Arc::new(..))` in
    dispatch/table.rs), with `//` comments stripped so doc examples like
    `"tool_name" => module::handler_fn` are not counted.
  - three_way_report(): classifies dispatch_only / schema_only /
    preset_dead, with a pending-D3 allowlist carve-out split into
    symbolic-edit-core and refactor-substrate buckets.
  - lint_description_crossrefs(): flags tool descriptions that name
    other tools (D7), honoring an explicit allowlist and word
    boundaries.
  - live-tree sentinels: parsing the real sources finds known tools,
    excludes comment artifacts, and stays above a sanity floor so a
    registration-style change breaks loudly.
"""
from __future__ import annotations

import importlib.util
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
_SPEC = importlib.util.spec_from_file_location(
    "regen_tool_defs", REPO_ROOT / "scripts" / "regen-tool-defs.py"
)
_MOD = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_MOD)

parse_dispatch_names = _MOD.parse_dispatch_names
three_way_report = _MOD.three_way_report
lint_description_crossrefs = _MOD.lint_description_crossrefs
collect_dispatch_names = _MOD.collect_dispatch_names


MATCH_ARM_FIXTURE = """
/// Declarative tool registry macro — reduces boilerplate and prevents drift.
/// Each entry is `"tool_name" => module::handler_fn`.
pub fn dispatch_table() -> HashMap<&'static str, ToolHandler> {
    tool_registry! {
        // ── File I/O ── ("commented_out" => nope,)
        "get_current_config"           => filesystem::get_current_config,
        "read_file"                    => filesystem::read_file_tool,
        "bm25_symbol_search"          => symbols::bm25_symbol_search,
    }
}
"""

INSERT_FIXTURE = """
pub(crate) static DISPATCH_TABLE: LazyLock<HashMap<&'static str, ToolHandler>> =
    LazyLock::new(|| {
        let mut m = tools::dispatch_table();
        m.insert(
            "semantic_search",
            std::sync::Arc::new(semantic_search_handler),
        );
        m.insert("classify_symbol", std::sync::Arc::new(classify_symbol_handler));
        // m.insert("disabled_tool", std::sync::Arc::new(nope));
        m
    });

fn some_handler() {
    // serde_json map inserts inside handler bodies must NOT count as
    // dispatch registrations (live false positives: provenance, unknown_args)
    payload.insert("provenance", json!("scip"));
    meta.insert(
        "unknown_args",
        serde_json::Value::Array(unknown),
    );
}
"""


def test_parse_match_arms_strips_comments() -> None:
    names = parse_dispatch_names(MATCH_ARM_FIXTURE)
    assert names == {
        "get_current_config",
        "read_file",
        "bm25_symbol_search",
    }, f"unexpected: {sorted(names)}"
    assert "tool_name" not in names, "doc-comment example must be stripped"
    assert "commented_out" not in names, "inline comment must be stripped"


def test_parse_insert_style_multiline_and_inline() -> None:
    names = parse_dispatch_names(INSERT_FIXTURE)
    assert names == {
        "semantic_search",
        "classify_symbol",
    }, f"unexpected: {sorted(names)}"
    assert "disabled_tool" not in names, "commented insert must be stripped"
    assert "provenance" not in names, "serde_json map insert must not count"
    assert "unknown_args" not in names, "serde_json map insert must not count"


def test_three_way_report_classifies() -> None:
    report = three_way_report(
        dispatch={"a", "b", "ghost"},
        schema={"a", "b", "schema_orphan"},
        preset_members={"a", "dead_preset_entry"},
    )
    assert report["dispatch_only"] == ["ghost"]
    assert report["schema_only"] == ["schema_orphan"]
    assert report["preset_dead"] == ["dead_preset_entry"]
    assert report["allowlisted_dispatch_only"] == []


def test_three_way_report_allowlist_carveout() -> None:
    report = three_way_report(
        dispatch={"a", "rename_symbol", "refactor_extract_function", "ghost"},
        schema={"a"},
        preset_members={"a", "rename_symbol", "refactor_extract_function"},
        dispatch_only_allowlist={"rename_symbol", "refactor_extract_function"},
    )
    assert report["dispatch_only"] == ["ghost"]
    assert report["allowlisted_dispatch_only"] == [
        "refactor_extract_function",
        "rename_symbol",
    ]
    assert report["pending_d3_symbolic_edit_core"] == ["rename_symbol"]
    assert report["pending_d3_refactor_substrate"] == ["refactor_extract_function"]
    # pending-D3 names may sit in preset constants (callable-but-unlisted)
    assert report["preset_dead"] == []


def test_lint_flags_crossref() -> None:
    tools = [
        {"name": "alpha_tool", "description": "Use beta_tool first, then this."},
        {"name": "beta_tool", "description": "Standalone description."},
    ]
    offenses = lint_description_crossrefs(tools)
    assert len(offenses) == 1, f"unexpected: {offenses}"
    assert "alpha_tool" in offenses[0] and "beta_tool" in offenses[0]


def test_lint_honors_allowlist_self_and_word_boundary() -> None:
    tools = [
        # self-mention is fine
        {"name": "alpha_tool", "description": "alpha_tool does X."},
        # substring of another name must NOT flag (word boundary)
        {"name": "beta", "description": "betamax is unrelated."},
        {"name": "betamax_x", "description": "plain."},
        # allowlisted pairs must NOT flag (allowlist is directional:
        # (tool, referenced_tool) — register each direction explicitly)
        {"name": "gamma", "description": "pairs with delta."},
        {"name": "delta", "description": "see gamma for setup."},
    ]
    offenses = lint_description_crossrefs(
        tools, allowlist={("gamma", "delta"), ("delta", "gamma")}
    )
    assert offenses == [], f"unexpected: {offenses}"


def test_live_tree_sentinels() -> None:
    names = collect_dispatch_names()
    # macro match-arm style
    assert "get_current_config" in names
    # multi-line .insert style (feature-gated semantic registrations)
    assert "semantic_search" in names
    # known ghost (dispatch-only today) — proves we parse dispatch, not toml
    assert "rename_symbol" in names
    # doc-comment artifact must not leak
    assert "tool_name" not in names
    # serde_json payload inserts in handler bodies must not leak
    assert "provenance" not in names
    assert "unknown_args" not in names
    # sanity floor: both files parsed (style drift breaks loudly).
    # 99 dispatch names after the #346 line-edit tombstones; the floor
    # only needs to catch a parser/style break (which drops to ~0).
    assert len(names) >= 90, f"suspiciously few dispatch names: {len(names)}"


# ── Phase 2: enforce-mode + tombstone re-introduction guard ────────────

enforce_failures = getattr(_MOD, "enforce_failures", None)
extract_tombstones = getattr(_MOD, "extract_tombstones", None)

EMPTY_REPORT = {
    "dispatch_only": [],
    "allowlisted_dispatch_only": [],
    "pending_d3_symbolic_edit_core": [],
    "pending_d3_refactor_substrate": [],
    "schema_only": [],
    "preset_dead": [],
}


def test_enforce_blocks_unexplained_drift() -> None:
    report = dict(EMPTY_REPORT, dispatch_only=["ghost"])
    fails = enforce_failures(report, lint_offenses=[], tombstone_hits=[])
    assert fails, "dispatch_only ghosts must block enforce mode"


def test_enforce_allows_allowlisted_only() -> None:
    report = dict(EMPTY_REPORT, allowlisted_dispatch_only=["rename_symbol"])
    assert enforce_failures(report, lint_offenses=[], tombstone_hits=[]) == []


def test_enforce_blocks_lint_and_tombstone_hits() -> None:
    fails = enforce_failures(
        dict(EMPTY_REPORT),
        lint_offenses=["x: description references tool `y`"],
        tombstone_hits=["replace re-introduced in dispatch"],
    )
    assert len(fails) == 2


TOMBSTONE_FIXTURE = """
pub(crate) const TOMBSTONED_TOOLS: &[(&str, &str)] = &[
    // line-edit family (#346)
    ("replace", "use host-native Edit"),
    (
        "insert_at_line",
        "use host-native Edit/Write",
    ),
];
"""


def test_extract_tombstones_parses_tuple_list() -> None:
    names = extract_tombstones(TOMBSTONE_FIXTURE)
    assert names == {"replace", "insert_at_line"}, f"unexpected: {sorted(names)}"


def main() -> int:
    failures: list[str] = []
    tests = [
        test_parse_match_arms_strips_comments,
        test_parse_insert_style_multiline_and_inline,
        test_three_way_report_classifies,
        test_three_way_report_allowlist_carveout,
        test_lint_flags_crossref,
        test_lint_honors_allowlist_self_and_word_boundary,
        test_live_tree_sentinels,
        test_enforce_blocks_unexplained_drift,
        test_enforce_allows_allowlisted_only,
        test_enforce_blocks_lint_and_tombstone_hits,
        test_extract_tombstones_parses_tuple_list,
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
