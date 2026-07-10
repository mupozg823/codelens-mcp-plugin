#!/usr/bin/env python3
"""Adversarial regression tests for Rust SCIP split acceptance."""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from test_productivity_study_acceptance_scip import (
    _check,
    _rewrite,
    write_scip_candidate,
)


def test_scip_split_rejects_conditionally_compiled_required_items() -> None:
    mutations = (
        (
            "mod.rs",
            "mod call_graph;\nmod navigation;\n#[cfg(any())]\nmod parse;\n"
            "#[cfg(test)]\nmod tests;\npub struct ScipBackend {}\n",
        ),
        (
            "mod.rs",
            "mod call_graph;\nmod navigation;\nmod parse;\n"
            "#[cfg(any())]\n#[cfg(test)]\nmod tests;\npub struct ScipBackend {}\n",
        ),
        (
            "call_graph.rs",
            "impl ScipBackend {\n"
            "pub fn find_callees(&self) {}\n"
            "#[cfg(any())]\npub fn find_callers(&self) {}\n}\n",
        ),
        ("tests.rs", "#[ignore]\n#[test]\nfn split_works() {}\n"),
        (
            "mod.rs",
            "#![cfg(any())]\nmod call_graph;\nmod navigation;\nmod parse;\n"
            "#[cfg(test)]\nmod tests;\npub struct ScipBackend {}\n",
        ),
        (
            "parse.rs",
            "#![cfg(any())]\n"
            "pub(super) fn short_name() {}\n"
            "pub(super) fn is_definition() {}\n"
            "pub(super) fn parse_range() {}\n"
            "pub(super) fn is_function_like_symbol() {}\n"
            "pub(super) fn body_end_line() {}\n",
        ),
        ("tests.rs", "#![cfg(any())]\n#[test]\nfn split_works() {}\n"),
        (
            "tests.rs",
            "#![allow(dead_code)] #![cfg(any())]\n#[test]\nfn split_works() {}\n",
        ),
        ("tests.rs", "#![r#cfg(any())]\n#[test]\nfn split_works() {}\n"),
        (
            "tests.rs",
            "#![r#cfg_attr(all(), cfg(any()))]\n#[test]\nfn split_works() {}\n",
        ),
    )
    for name, source in mutations:
        with tempfile.TemporaryDirectory(prefix="study-scip-cfg-") as raw_tmp:
            candidate = Path(raw_tmp)
            write_scip_candidate(candidate)
            _rewrite(candidate, name, source)
            result = _check(candidate)

        assert result[0] is False, name


def test_scip_split_rejects_spaced_or_commented_cfg_attributes() -> None:
    attributes = ("# [cfg(any())]", "#\n[cfg(any())]", "# /* gap */ [cfg(any())]")
    for attribute in attributes:
        with tempfile.TemporaryDirectory(prefix="study-scip-cfg-spacing-") as raw_tmp:
            candidate = Path(raw_tmp)
            write_scip_candidate(candidate)
            _rewrite(
                candidate,
                "call_graph.rs",
                "impl ScipBackend {\n"
                f"{attribute}\npub fn find_callees(&self) {{}}\n"
                "pub fn find_callers(&self) {}\n}\n",
            )
            result = _check(candidate)

        assert result[0] is False, attribute


def test_scip_split_requires_a_top_level_live_unit_test() -> None:
    with tempfile.TemporaryDirectory(prefix="study-scip-test-scope-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_scip_candidate(candidate)
        _rewrite(
            candidate,
            "tests.rs",
            "#[cfg(any())]\nmod disabled {\n#[test]\nfn split_works() {}\n}\n",
        )
        result = _check(candidate)

    assert result[0] is False
    assert "unit tests" in (result[1] or "")


def test_scip_split_rejects_unexpanded_macro_witnesses() -> None:
    with tempfile.TemporaryDirectory(prefix="study-scip-macro-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_scip_candidate(candidate)
        _rewrite(
            candidate,
            "mod.rs",
            "macro_rules! fake ( () => (\n"
            "mod call_graph;\nmod navigation;\nmod parse;\n"
            "#[cfg(test)]\nmod tests;\npub struct ScipBackend;\n); );\n",
        )
        _rewrite(
            candidate,
            "parse.rs",
            "macro_rules! fake ( () => (\n"
            "pub(super) fn short_name();\npub(super) fn is_definition();\n"
            "pub(super) fn parse_range();\n"
            "pub(super) fn is_function_like_symbol();\n"
            "pub(super) fn body_end_line();\n); );\n",
        )
        _rewrite(
            candidate,
            "navigation.rs",
            "macro_rules! fake ( () => ( impl PreciseBackend for ScipBackend {} ); );\n",
        )
        _rewrite(
            candidate,
            "call_graph.rs",
            "macro_rules! fake ( () => ( impl ScipBackend {\n"
            "pub fn find_callees(&self) {}\npub fn find_callers(&self) {}\n} ); );\n",
        )
        _rewrite(
            candidate,
            "tests.rs",
            "macro_rules! fake ( () => (\n#[test]\nfn split_works() {}\n); );\n",
        )
        result = _check(candidate)

    assert result[0] is False


def main() -> int:
    tests = [value for name, value in globals().items() if name.startswith("test_")]
    failures = 0
    for test in tests:
        try:
            test()
            print(f"PASS  {test.__name__}")
        except AssertionError as error:
            failures += 1
            print(f"FAIL  {test.__name__}: {error}")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
