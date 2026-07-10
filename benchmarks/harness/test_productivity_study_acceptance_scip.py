#!/usr/bin/env python3
"""Regression tests for evaluator-owned Rust SCIP split acceptance."""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import productivity_study_acceptance as acceptance


_SCIP_ROOT = "crates/codelens-engine/src/scip_backend"
_HELPERS = (
    "short_name",
    "is_definition",
    "parse_range",
    "is_function_like_symbol",
    "body_end_line",
)


def write(root: Path, relative: str, content: str) -> None:
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def write_scip_candidate(root: Path, *, wire_tests: bool = True) -> None:
    test_wiring = "#[cfg(test)]\nmod tests;\n" if wire_tests else ""
    write(
        root,
        f"{_SCIP_ROOT}/mod.rs",
        "mod call_graph;\nmod navigation;\nmod parse;\n"
        f"{test_wiring}pub struct ScipBackend {{}}\n",
    )
    helpers = "\n".join(f"pub(super) fn {name}() {{}}" for name in _HELPERS)
    write(root, f"{_SCIP_ROOT}/parse.rs", helpers)
    write(root, f"{_SCIP_ROOT}/navigation.rs", "impl PreciseBackend for ScipBackend {}\n")
    write(
        root,
        f"{_SCIP_ROOT}/call_graph.rs",
        "impl ScipBackend {\n"
        "pub fn find_callees(&self) {}\n"
        "pub fn find_callers(&self) {}\n}\n",
    )
    write(root, f"{_SCIP_ROOT}/tests.rs", "#[test]\nfn split_works() {}\n")


def _check(candidate: Path) -> tuple[bool, str | None]:
    return acceptance.run_evaluator_checks(candidate, (acceptance.CODELENS_SCIP_SPLIT_ID,))


def _rewrite(candidate: Path, name: str, source: str) -> None:
    write(candidate, f"{_SCIP_ROOT}/{name}", source)


def test_scip_split_rejects_base_noop_and_accepts_target_shape() -> None:
    with tempfile.TemporaryDirectory(prefix="study-scip-") as raw_tmp:
        candidate = Path(raw_tmp)
        write(candidate, "crates/codelens-engine/src/scip_backend.rs", "pub struct ScipBackend;\n")
        assert _check(candidate)[0] is False
        (candidate / "crates/codelens-engine/src/scip_backend.rs").unlink()
        write_scip_candidate(candidate)
        result = _check(candidate)

    assert result == (True, None)


def test_scip_split_rejects_missing_test_wiring() -> None:
    with tempfile.TemporaryDirectory(prefix="study-scip-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_scip_candidate(candidate, wire_tests=False)
        result = _check(candidate)

    assert result[0] is False
    assert "test module wiring" in (result[1] or "")


def test_scip_split_ignores_rust_comments_and_literals() -> None:
    mutations = (
        (
            "mod.rs",
            "mod call_graph;\nmod navigation;\n// mod parse;\n"
            "#[cfg(test)]\nmod tests;\npub struct ScipBackend {}\n",
        ),
        (
            "mod.rs",
            "mod call_graph;\nmod navigation;\nmod parse;\n#[cfg(test)]\nmod tests;\n"
            'const TRAP: &str = r#"pub struct ScipBackend {}"#;\n',
        ),
        (
            "mod.rs",
            "mod call_graph;\nmod navigation;\nmod parse;\n#[cfg(test)]\nmod tests;\n"
            "struct ScipBackend {}\n",
        ),
        (
            "parse.rs",
            "// pub(super) fn short_name() {}\n"
            "pub(super) fn is_definition() {}\n"
            "pub(super) fn parse_range() {}\n"
            "pub(super) fn is_function_like_symbol() {}\n"
            "pub(super) fn body_end_line() {}\n",
        ),
        (
            "parse.rs",
            "fn short_name() {}\n"
            "pub(super) fn is_definition() {}\n"
            "pub(super) fn parse_range() {}\n"
            "pub(super) fn is_function_like_symbol() {}\n"
            "pub(super) fn body_end_line() {}\n",
        ),
        (
            "navigation.rs",
            'const TRAP: &str = "impl PreciseBackend for ScipBackend {}";\n',
        ),
        (
            "navigation.rs",
            'const TRAP: &[u8] = b"impl PreciseBackend for ScipBackend {}";\n',
        ),
        (
            "navigation.rs",
            'const TRAP: &[u8] = br###"impl PreciseBackend for ScipBackend {}"###;\n',
        ),
        (
            "tests.rs",
            'const TRAP: &str = r##"#[test]\nfn split_works() {}"##;\n',
        ),
    )
    for name, source in mutations:
        with tempfile.TemporaryDirectory(prefix="study-scip-lexical-") as raw_tmp:
            candidate = Path(raw_tmp)
            write_scip_candidate(candidate)
            _rewrite(candidate, name, source)
            result = _check(candidate)

        assert result[0] is False, name


def test_scip_split_requires_test_attribute_to_wire_a_function() -> None:
    with tempfile.TemporaryDirectory(prefix="study-scip-test-wiring-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_scip_candidate(candidate)
        _rewrite(
            candidate,
            "tests.rs",
            "#[test]\nconst MARKER: bool = true;\nfn split_works() {}\n",
        )
        result = _check(candidate)

    assert result[0] is False
    assert "unit tests" in (result[1] or "")


def test_scip_split_requires_call_graph_methods_inside_inherent_impl() -> None:
    with tempfile.TemporaryDirectory(prefix="study-scip-call-graph-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_scip_candidate(candidate)
        _rewrite(
            candidate,
            "call_graph.rs",
            "pub fn find_callers() {}\n"
            "impl Other { pub fn find_callees(&self) {} }\n"
            "impl ScipBackend {}\n",
        )
        result = _check(candidate)

    assert result[0] is False
    assert "call graph method" in (result[1] or "")


def test_scip_split_requires_cfg_attribute_to_directly_precede_test_module() -> None:
    with tempfile.TemporaryDirectory(prefix="study-scip-module-wiring-") as raw_tmp:
        candidate = Path(raw_tmp)
        write_scip_candidate(candidate)
        _rewrite(
            candidate,
            "mod.rs",
            "mod call_graph;\nmod navigation;\nmod parse;\n"
            "#[cfg(test)]\nconst TESTS_ENABLED: bool = true;\nmod tests;\n"
            "pub struct ScipBackend {}\n",
        )
        result = _check(candidate)

    assert result[0] is False
    assert "test module wiring" in (result[1] or "")


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
        (
            "tests.rs",
            "#[ignore]\n#[test]\nfn split_works() {}\n",
        ),
    )
    for name, source in mutations:
        with tempfile.TemporaryDirectory(prefix="study-scip-cfg-") as raw_tmp:
            candidate = Path(raw_tmp)
            write_scip_candidate(candidate)
            _rewrite(candidate, name, source)
            result = _check(candidate)

        assert result[0] is False, name


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
