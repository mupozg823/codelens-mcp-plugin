"""Study execution wiring with treatment-blind review artifacts."""

from __future__ import annotations

import json
import subprocess
from dataclasses import dataclass
from pathlib import Path

from productivity_study_agents import AgentInvocation
from productivity_study_contract import (
    EvidenceRetention,
    IndexMode,
    QualityStatus,
    RunStatus,
    StudyIdentity,
    StudyManifest,
    VerificationStatus,
    blind_review_id_for,
    retain_minimal_evidence,
)
from productivity_study_mcp_metrics import aggregate_agent_metrics
from productivity_study_runtime import (
    DaemonResourceMonitor,
    build_daemon_command,
    dedicated_daemon,
    metrics_snapshot,
    open_mcp_session,
    mcp_tool_call,
    run_agent,
    runtime_cpu_millis,
)
from productivity_study_runner import (
    CandidateGrade,
    PlannedRun,
    PolicySnapshot,
    WorktreeRequest,
    candidate_changed_paths,
    disposable_worktree,
    grade_candidate,
)


@dataclass(frozen=True, slots=True)
class StudyExecutionConfig:
    study_id: str
    artifact_root: Path
    policy_path: Path
    codelens_repo: Path
    codelens_binary: Path
    index_mode: IndexMode
    codex_model: str
    claude_model: str
    timeout_seconds: int


def run_id_for(planned: PlannedRun) -> str:
    return f"{planned.sequence_order:03d}-{planned.task.task_id.replace('::', '-')}-{planned.agent.value}-{planned.condition.value}"


def build_blind_review_packet(
    run_id: str, planned: PlannedRun, response: str
) -> dict[str, object]:
    return {
        "review_id": blind_review_id_for(run_id),
        "task_id": planned.task.task_id,
        "task_kind": planned.task.task_kind,
        "prompt": planned.task.prompt,
        "rubric": list(planned.task.hidden_rubric),
        "response": response,
    }


def execute_planned_run(planned: PlannedRun, config: StudyExecutionConfig) -> dict[str, object]:
    run_id = run_id_for(planned)
    record_dir = config.artifact_root / config.study_id / run_id
    record_dir.mkdir(parents=True, exist_ok=False)
    policy = PolicySnapshot.capture(config.policy_path)
    identity = StudyIdentity(
        study_id=config.study_id,
        scenario_id=planned.task.task_id,
        task_kind=planned.task.task_kind,
        agent=planned.agent,
        model=model_for(planned, config),
        cli_version=cli_version(planned.agent.value),
        condition=planned.condition,
        repo_id=planned.task.repo_id,
        repo_path=planned.task.repo_path,
        base_sha=planned.task.base_sha,
        target_sha=planned.task.target_sha,
        codelens_sha=git_commit(config.codelens_repo),
        codelens_binary=config.codelens_binary,
        policy_sha=policy.sha256,
        index_mode=config.index_mode,
        sequence_order=planned.sequence_order,
    )
    manifest = StudyManifest.create(identity).to_payload()
    raw_path = record_dir / "agent-stream.raw"
    request = WorktreeRequest(
        planned.task.repo_path,
        planned.task.base_sha,
        config.artifact_root / "worktrees" / config.study_id,
        f"{run_id}-candidate",
    )
    with disposable_worktree(request) as candidate:
        result = execute_in_worktree(planned, config, candidate, raw_path)
        grade = grade_for(planned, config, candidate, run_id)
    unchanged_policy = policy.matches(config.policy_path)
    retention = retain_minimal_evidence(raw_path, result["failure_excerpt"])
    telemetry_path = result.pop("mcp_telemetry_path", None)
    telemetry_retention = retain_minimal_evidence(Path(str(telemetry_path)), result["failure_excerpt"]) if telemetry_path is not None and Path(str(telemetry_path)).is_file() else None
    response = str(result.pop("response"))
    manifest.update(
        {
            "status": RunStatus.COMPLETED.value if unchanged_policy else RunStatus.INVALID.value,
            "quality_status": quality_status(planned, grade, unchanged_policy).value,
            "verification_status": verification_status(planned, grade).value,
            "result": result,
            "candidate_grade": grade_payload(grade),
            "policy_unchanged": unchanged_policy,
            "raw_evidence": retention_payload(retention),
            "mcp_raw_evidence": retention_payload(telemetry_retention) if telemetry_retention is not None else None,
        }
    )
    if planned.task.read_only:
        manifest["blind_review"] = {"id": blind_review_id_for(run_id), "status": "pending"}
        write_blind_packet(record_dir, run_id, planned, response)
    manifest_path = record_dir / "run-manifest.json"
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    return manifest


def execute_in_worktree(
    planned: PlannedRun,
    config: StudyExecutionConfig,
    candidate: Path,
    raw_path: Path,
) -> dict[str, object]:
    mcp_config = raw_path.with_name("claude-mcp.json")
    if planned.condition.value == "baseline":
        return run_agent(
            invocation_for(planned, config, candidate, "", mcp_config),
            candidate,
            raw_path,
            config.timeout_seconds,
        )
    telemetry_path = raw_path.with_name("mcp-telemetry.raw")
    with dedicated_daemon(config.codelens_binary, candidate, telemetry_path) as daemon:
        control_session = open_mcp_session(daemon.url)
        if config.index_mode is IndexMode.WARM:
            mcp_tool_call(
                daemon.url,
                control_session,
                "prepare_harness_session",
                {"project": str(candidate), "detail": "compact"},
            )
        resources = DaemonResourceMonitor(daemon.pid)
        result = run_agent(
            invocation_for(planned, config, candidate, daemon.url, mcp_config),
            candidate,
            raw_path,
            config.timeout_seconds,
            resources.sample,
        )
        result["mcp_metrics"] = aggregate_agent_metrics(
            telemetry_path,
            (daemon.health_session_id, control_session),
            lambda agent_session: metrics_snapshot(daemon.url, control_session, agent_session),
            resources.summary(),
            daemon.startup_ms,
        )
        result["mcp_telemetry_path"] = str(telemetry_path)
        return result


def invocation_for(
    planned: PlannedRun,
    config: StudyExecutionConfig,
    candidate: Path,
    mcp_url: str,
    mcp_config: Path,
) -> AgentInvocation:
    mcp_config.write_text(
        json.dumps({"mcpServers": {"codelens": {"type": "http", "url": mcp_url}}}),
        encoding="utf-8",
    )
    return AgentInvocation(
        agent=planned.agent,
        condition=planned.condition,
        prompt=planned.task.prompt,
        worktree=candidate,
        model=model_for(planned, config),
        read_only=planned.task.read_only,
        codelens_url=mcp_url,
        claude_mcp_config=mcp_config,
        routed_policy=policy_excerpt(config.policy_path, planned),
    )


def grade_for(
    planned: PlannedRun, config: StudyExecutionConfig, candidate: Path, run_id: str
) -> CandidateGrade:
    if planned.task.read_only:
        changed_paths = candidate_changed_paths(candidate)
        accepted = not changed_paths
        return CandidateGrade(accepted, accepted, accepted, changed_paths, None)
    evaluator_request = WorktreeRequest(
        planned.task.repo_path,
        planned.task.target_sha,
        config.artifact_root / "worktrees" / config.study_id,
        f"{run_id}-evaluator",
    )
    with disposable_worktree(evaluator_request) as evaluator:
        return grade_candidate(
            candidate,
            evaluator,
            hidden_test_paths=planned.task.hidden_test_paths,
            verification_commands=planned.task.verification_commands,
            allowed_paths=planned.task.allowed_paths,
        )


def model_for(planned: PlannedRun, config: StudyExecutionConfig) -> str:
    return config.codex_model if planned.agent.value == "codex" else config.claude_model


def cli_version(agent_name: str) -> str:
    completed = subprocess.run(
        [agent_name, "--version"], check=True, capture_output=True, text=True
    )
    return completed.stdout.strip()


def git_commit(repo: Path) -> str:
    completed = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=repo, check=True, capture_output=True, text=True
    )
    return completed.stdout.strip()


def quality_status(
    planned: PlannedRun, grade: CandidateGrade, policy_unchanged: bool
) -> QualityStatus:
    if not policy_unchanged or planned.task.read_only:
        return QualityStatus.HELD
    return QualityStatus.PASSED if grade.accepted else QualityStatus.FAILED


def verification_status(planned: PlannedRun, grade: CandidateGrade) -> VerificationStatus:
    if planned.task.read_only:
        return VerificationStatus.NOT_APPLICABLE if grade.accepted else VerificationStatus.FAILED
    return VerificationStatus.PASSED if grade.verification_passed else VerificationStatus.FAILED


def grade_payload(grade: CandidateGrade) -> dict[str, object]:
    return {"accepted": grade.accepted, "allowed_diff": grade.allowed_diff, "verification_passed": grade.verification_passed, "changed_paths": list(grade.changed_paths), "failure_excerpt": grade.failure_excerpt}


def retention_payload(retention: EvidenceRetention) -> dict[str, object]:
    return {"sha256": retention.sha256, "raw_deleted": retention.raw_deleted, "failure_excerpt": retention.failure_excerpt}


def write_blind_packet(
    record_dir: Path, run_id: str, planned: PlannedRun, response: str
) -> None:
    packet_path = record_dir / "blind-review-packet.json"
    packet_path.write_text(json.dumps(build_blind_review_packet(run_id, planned, response), ensure_ascii=False, indent=2) + "\n")


def policy_excerpt(policy_path: Path, planned: PlannedRun) -> str:
    policy = json.loads(policy_path.read_text(encoding="utf-8"))
    task_kind = {
        "simple-local-lookup": "simple local lookup/edit",
        "multi-file-impact-review": "impact/reviewer",
        "defect-repair": "impact/reviewer",
        "safe-refactor": "refactor preflight",
    }[planned.task.task_kind]
    repo_id = {"signaturestudio": "signature-studio"}.get(planned.task.repo_id, planned.task.repo_id)
    rules = list(policy.get("repo_overrides", [])) + list(policy.get("global_rules", []))
    for rule in rules:
        if isinstance(rule, dict) and rule.get("task_kind") == task_kind:
            if rule.get("repo_id") in {None, repo_id}:
                return f"{rule.get('recommended_policy')}: {rule.get('explanation')}"
    raise RuntimeError(f"no fixed routing policy for {repo_id}/{task_kind}")
