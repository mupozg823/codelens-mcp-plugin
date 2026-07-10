"""Immutable evidence contract for controlled CodeLens productivity studies."""

from __future__ import annotations

import hashlib
from dataclasses import dataclass
from datetime import UTC, datetime
from enum import StrEnum
from pathlib import Path
from typing import TypedDict


class Agent(StrEnum):
    CODEX = "codex"
    CLAUDE = "claude"


class Condition(StrEnum):
    BASELINE = "baseline"
    NAIVE = "naive-on"
    ROUTED = "routed-on"


class IndexMode(StrEnum):
    COLD = "cold"
    WARM = "warm"


class RunStatus(StrEnum):
    PLANNED = "planned"
    COMPLETED = "completed"
    INVALID = "invalid"


class QualityStatus(StrEnum):
    PENDING = "pending"
    PASSED = "passed"
    FAILED = "failed"
    HELD = "held"


class VerificationStatus(StrEnum):
    PENDING = "pending"
    PASSED = "passed"
    FAILED = "failed"
    NOT_APPLICABLE = "not-applicable"


class StudyIdentityPayload(TypedDict):
    study_id: str
    scenario_id: str
    task_kind: str
    agent: str
    model: str
    cli_version: str
    condition: str
    repo_id: str
    repo_path: str
    base_sha: str
    target_sha: str
    codelens_sha: str
    codelens_binary: str
    policy_sha: str
    index_mode: str
    sequence_order: int


@dataclass(frozen=True, slots=True)
class StudyIdentity:
    study_id: str
    scenario_id: str
    task_kind: str
    agent: Agent
    model: str
    cli_version: str
    condition: Condition
    repo_id: str
    repo_path: Path
    base_sha: str
    target_sha: str
    codelens_sha: str
    codelens_binary: Path
    policy_sha: str
    index_mode: IndexMode
    sequence_order: int

    def payload(self) -> StudyIdentityPayload:
        return {
            "study_id": self.study_id,
            "scenario_id": self.scenario_id,
            "task_kind": self.task_kind,
            "agent": self.agent.value,
            "model": self.model,
            "cli_version": self.cli_version,
            "condition": self.condition.value,
            "repo_id": self.repo_id,
            "repo_path": str(self.repo_path),
            "base_sha": self.base_sha,
            "target_sha": self.target_sha,
            "codelens_sha": self.codelens_sha,
            "codelens_binary": str(self.codelens_binary),
            "policy_sha": self.policy_sha,
            "index_mode": self.index_mode.value,
            "sequence_order": self.sequence_order,
        }


@dataclass(frozen=True, slots=True)
class StudyManifest:
    identity: StudyIdentity
    status: RunStatus
    created_at: str
    quality_status: QualityStatus
    verification_status: VerificationStatus

    @classmethod
    def create(cls, identity: StudyIdentity) -> StudyManifest:
        return cls(
            identity=identity,
            status=RunStatus.PLANNED,
            created_at=datetime.now(UTC).isoformat(),
            quality_status=QualityStatus.PENDING,
            verification_status=VerificationStatus.PENDING,
        )

    def identity_mismatches(self, candidate: StudyIdentity) -> tuple[str, ...]:
        expected = self.identity.payload()
        actual = candidate.payload()
        return tuple(key for key in expected if expected[key] != actual[key])

    def to_payload(self) -> dict[str, object]:
        return {
            "schema_version": "productivity-study-v1",
            "identity": self.identity.payload(),
            "status": self.status.value,
            "created_at": self.created_at,
            "policy_mutation": "forbidden",
            "quality_status": self.quality_status.value,
            "verification_status": self.verification_status.value,
        }


@dataclass(frozen=True, slots=True)
class EvidenceRetention:
    sha256: str
    failure_excerpt: str | None
    raw_deleted: bool


def blind_review_id_for(run_id: str) -> str:
    return hashlib.sha256(run_id.encode("utf-8")).hexdigest()[:16]


def retain_minimal_evidence(raw_path: Path, failure_excerpt: str | None) -> EvidenceRetention:
    digest = hashlib.sha256(raw_path.read_bytes()).hexdigest()
    raw_path.unlink()
    return EvidenceRetention(
        sha256=digest,
        failure_excerpt=failure_excerpt,
        raw_deleted=True,
    )
