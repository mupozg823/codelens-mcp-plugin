#!/usr/bin/env python3
"""Contract tests for immutable productivity-study evidence."""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import productivity_study_contract as study


def identity(condition: study.Condition = study.Condition.BASELINE) -> study.StudyIdentity:
    return study.StudyIdentity(
        study_id="pilot-v1",
        scenario_id="codelens::impact-review::01",
        task_kind="multi-file-impact-review",
        agent=study.Agent.CODEX,
        model="gpt-5",
        cli_version="0.144.1",
        condition=condition,
        repo_id="codelens-mcp-plugin",
        repo_path=Path("/tmp/codelens"),
        base_sha="a" * 40,
        target_sha="b" * 40,
        codelens_sha="c" * 40,
        codelens_binary=Path("/tmp/codelens-mcp"),
        policy_sha="d" * 64,
        index_mode=study.IndexMode.WARM,
        sequence_order=2,
    )


def test_manifest_rejects_reused_run_dir_for_different_condition() -> None:
    given = identity()
    manifest = study.StudyManifest.create(given)

    when = identity(study.Condition.ROUTED)
    mismatch = manifest.identity_mismatches(when)

    assert mismatch == ("condition",)


def test_raw_transcript_is_removed_after_minimal_evidence_retention() -> None:
    with tempfile.TemporaryDirectory(prefix="codelens-study-contract-") as raw_tmp:
        root = Path(raw_tmp)
        raw_transcript = root / "agent-events.jsonl"
        raw_transcript.write_text('{"secret":"source body"}\n', encoding="utf-8")

        retained = study.retain_minimal_evidence(raw_transcript, "tool call failed")

        assert raw_transcript.exists() is False
        assert retained.sha256
        assert retained.failure_excerpt == "tool call failed"
        assert retained.raw_deleted is True


def test_manifest_serializes_required_identity_and_immutable_policy_state() -> None:
    manifest = study.StudyManifest.create(identity())

    payload = manifest.to_payload()

    assert payload["schema_version"] == "productivity-study-v1"
    assert payload["identity"]["study_id"] == "pilot-v1"
    assert payload["identity"]["target_sha"] == "b" * 40
    assert payload["identity"]["task_kind"] == "multi-file-impact-review"
    assert payload["policy_mutation"] == "forbidden"
    assert payload["quality_status"] == "pending"
    assert payload["verification_status"] == "pending"


def main() -> int:
    tests = [
        test_manifest_rejects_reused_run_dir_for_different_condition,
        test_raw_transcript_is_removed_after_minimal_evidence_retention,
        test_manifest_serializes_required_identity_and_immutable_policy_state,
    ]
    failures = 0
    for test in tests:
        try:
            test()
            print(f"PASS  {test.__name__}")
        except Exception as error:
            failures += 1
            print(f"FAIL  {test.__name__}: {error}")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
