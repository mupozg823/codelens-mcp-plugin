#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

# --- How to run ---
# 1. Install uv (if not installed):
#      curl -LsSf https://astral.sh/uv/install.sh | sh
# 2. Run directly:
#      uv run scripts/test/test-public-release-channel-smoke.py
# 3. CI can also run it with system Python:
#      python3 scripts/test/test-public-release-channel-smoke.py
# ------------------

from __future__ import annotations

import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

from public_release_channel_smoke import (  # noqa: E402
    ReleaseChannelSmokeError,
    build_plan,
    render_plan,
    validate_checksums,
    validate_formula,
)


def formula_text(version: str) -> str:
    return "\n".join(
        [
            '  version "' + version + '"',
            '  url "https://github.com/mupozg823/codelens-mcp-plugin/releases/download/v#{version}/codelens-mcp-darwin-arm64.tar.gz"',
            '  url "https://github.com/mupozg823/codelens-mcp-plugin/releases/download/v#{version}/codelens-mcp-linux-x86_64.tar.gz"',
            '  sha256 "abc123"',
            '  prefix.install "models" if File.directory?("models")',
        ]
    )


def test_render_plan_is_side_effect_free_by_default() -> None:
    plan = build_plan("v1.2.3", "mupozg823/codelens-mcp-plugin", "mupozg823/homebrew-tap")

    rendered = render_plan(plan)

    assert "Public release-channel smoke plan for v1.2.3" in rendered
    assert "--mode metadata" in rendered
    assert "curl -fsSL https://github.com/mupozg823/codelens-mcp-plugin/releases/download/v1.2.3/checksums-sha256.txt" in rendered
    assert "brew info --json=v2 mupozg823/tap/codelens-mcp" in rendered


def test_validate_checksums_requires_all_release_assets() -> None:
    text = "\n".join(
        [
            "abc  codelens-mcp-darwin-arm64.tar.gz",
            "def  codelens-mcp-linux-x86_64.tar.gz",
            "ghi  codelens-mcp-windows-x86_64.zip",
            "jkl  release-manifest.json",
        ]
    )

    evidence = validate_checksums(text)

    assert len(evidence) == 4


def test_validate_formula_rejects_checksum_placeholders() -> None:
    plan = build_plan("1.2.3", "mupozg823/codelens-mcp-plugin", "mupozg823/homebrew-tap")

    try:
        validate_formula(formula_text("1.2.3") + "\nRELEASE_SHA256_DARWIN_ARM64", plan)
    except ReleaseChannelSmokeError as error:
        assert "checksum placeholders" in str(error)
        return
    raise AssertionError("formula placeholders should fail")


def test_validate_formula_requires_matching_version() -> None:
    plan = build_plan("1.2.3", "mupozg823/codelens-mcp-plugin", "mupozg823/homebrew-tap")

    try:
        validate_formula(formula_text("9.9.9"), plan)
    except ReleaseChannelSmokeError as error:
        assert 'version "1.2.3"' in str(error)
        return
    raise AssertionError("formula version mismatch should fail")


def main() -> int:
    tests = [
        test_render_plan_is_side_effect_free_by_default,
        test_validate_checksums_requires_all_release_assets,
        test_validate_formula_rejects_checksum_placeholders,
        test_validate_formula_requires_matching_version,
    ]
    failures = []
    for test in tests:
        try:
            test()
            print(f"PASS  {test.__name__}")
        except AssertionError as error:
            print(f"FAIL  {test.__name__}: {error}")
            failures.append(test.__name__)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
